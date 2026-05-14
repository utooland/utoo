use anyhow::Result;
use std::env;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use indicatif::{ProgressBar, ProgressStyle};
use once_cell::sync::{Lazy, OnceCell};
use owo_colors::OwoColorize;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{
    EnvFilter, Layer, Registry, fmt, layer::SubscriberExt, util::SubscriberInitExt,
};

/// Cached at startup: is stderr connected to a terminal?
///
/// indicatif's `inc()` / `set_message()` always acquire the internal
/// `Mutex<ProgressState>` write-lock and mutate `pos` / `est.buf` /
/// `message`, even when the draw target is hidden or the underlying
/// stream is not a TTY — only the *rendering* short-circuits, not the
/// state mutation. On the dependency-resolve hot path that's 9000+
/// lock acquisitions per phase contending across multiple workers,
/// costing measurable wall time on CI.
///
/// Gating every `ProgressReceiver` event behind this flag means
/// non-TTY environments (CI, piped output, `2>&1 | tee`) pay zero
/// indicatif cost — no `inc`, no `set_message`, no `format!()`
/// allocation. Local dev keeps the full progress UX.
pub static IS_TTY: Lazy<bool> = Lazy::new(|| std::io::stderr().is_terminal());

pub static PROGRESS_BAR: Lazy<ProgressBar> = Lazy::new(|| {
    let pb = ProgressBar::new(0).with_style(
        ProgressStyle::with_template("{spinner:.blue} +{pos:.green} ~{len:.magenta} {wide_msg}")
            .unwrap()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"),
    );
    pb.set_draw_target(indicatif::ProgressDrawTarget::hidden());
    pb
});

// Global state for tracing
static LOG_FILE_PATH: OnceCell<PathBuf> = OnceCell::new();

/// Initialize tracing subscriber with console and file output
/// Returns (log_path, guard) - the guard must be kept alive for the duration of the program
pub fn init_tracing(verbose: bool) -> Result<(PathBuf, WorkerGuard)> {
    // 1. Build environment filters
    // Note: Binary name is "utoo", so module paths start with "utoo::" not "utoo_pm::"

    // Console filter: verbose mode shows debug, otherwise show info+
    let console_level = if verbose { "debug" } else { "info" };
    let console_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(format!("utoo={console_level}")));

    // File filter: always capture debug+ for troubleshooting
    let file_filter = EnvFilter::new("utoo=debug");

    // 2. Create log file
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let log_path = env::temp_dir().join(format!("utoo-{timestamp}.log"));
    let file_appender =
        tracing_appender::rolling::never(env::temp_dir(), format!("utoo-{timestamp}.log"));
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    // Store log path for error reporting
    LOG_FILE_PATH.set(log_path.clone()).ok();

    // 3. Detect if stdout is a TTY (terminal) to decide on colors
    let is_tty = atty::is(atty::Stream::Stdout);

    // 4. Build subscriber with different filters for console and file
    Registry::default()
        .with(
            fmt::layer()
                .with_writer(std::io::stdout)
                .with_target(verbose) // Show module path only in verbose mode
                .without_time() // No timestamp on console
                .compact()
                .with_ansi(is_tty) // Enable colors only when output is to terminal
                .with_filter(console_filter),
        )
        .with(
            // File layer: always capture debug+ logs for troubleshooting
            fmt::layer()
                .with_writer(non_blocking)
                .with_target(true)
                .with_line_number(true)
                .with_thread_ids(true)
                .with_ansi(false) // Never use colors in log files
                .with_filter(file_filter),
        )
        .init();

    Ok((log_path, guard))
}

/// Get the path to the current log file
pub fn get_log_file_path() -> Option<&'static PathBuf> {
    LOG_FILE_PATH.get()
}

/// Finish the progress bar, optionally appending a dimmed `[2.6s]` suffix.
pub fn finish_progress_bar(msg: &str, elapsed: Option<Duration>) {
    if PROGRESS_BAR.length().unwrap_or(0) == 0 {
        return;
    }
    PROGRESS_BAR.set_style(
        ProgressStyle::with_template("✓ {pos:.green}/{len:.magenta} {wide_msg}").unwrap(),
    );
    let full_msg = match elapsed {
        Some(d) => format!("{msg} {}", format_elapsed_time(d).dimmed()),
        None => msg.to_string(),
    };
    PROGRESS_BAR.finish_with_message(full_msg);
    PROGRESS_BAR.set_draw_target(indicatif::ProgressDrawTarget::hidden());
    // reset color
    println!("\x1b[0m");
}

/// Print the install counts line, e.g.
/// `+ 513 added · 3017 reused · 123 downloaded`.
///
/// Semantics match pnpm:
/// - `added`: packages linked into `node_modules/` this run
/// - `reused`: tarballs served from the local cache (no network)
/// - `downloaded`: tarballs fetched from the registry
pub fn print_install_counts(added: usize, reused: usize, downloaded: usize) {
    println!(
        "+ {} {} · {} {} · {} {}",
        added.to_string().green(),
        "added".dimmed(),
        reused.to_string().magenta(),
        "reused".dimmed(),
        downloaded.to_string().cyan(),
        "downloaded".dimmed(),
    );
}

pub fn start_progress_bar() {
    if !*IS_TTY {
        return;
    }
    PROGRESS_BAR.reset();
    PROGRESS_BAR.set_style(
        ProgressStyle::with_template("{spinner:.blue} {pos:.green}/{len:.magenta} {wide_msg}")
            .unwrap()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"),
    );
    PROGRESS_BAR.set_draw_target(indicatif::ProgressDrawTarget::stderr());
    PROGRESS_BAR.enable_steady_tick(Duration::from_millis(100));
}

pub fn log_progress(text: &str) {
    if !*IS_TTY {
        return;
    }
    PROGRESS_BAR.set_message(text.to_string());
}

// Global timer for log_time/log_time_end
static START_TIME: OnceCell<Instant> = OnceCell::new();

/// Start the global timer for elapsed time logging.
pub fn log_time() {
    let _ = START_TIME.set(Instant::now());
}

/// Format a Duration like `1.5s`, `2m5s`, `1h2m3s`.
fn fmt_duration(elapsed: Duration) -> String {
    let total_secs = elapsed.as_secs();
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;

    if hours == 0 && minutes == 0 {
        return format!("{:.1}s", elapsed.as_secs_f64());
    }
    if hours > 0 {
        format!("{hours}h{minutes}m{seconds}s")
    } else {
        format!("{minutes}m{seconds}s")
    }
}

/// Format a Duration into `[1.5s]` / `[2m5s]` / `[1h2m3s]`.
pub fn format_elapsed_time(elapsed: Duration) -> String {
    format!("[{}]", fmt_duration(elapsed))
}

/// End the global timer and print elapsed time with a message.
/// Example output: "75 packages installed [5s]"
pub fn log_time_end(msg: &str) {
    println!();
    if let Some(start) = START_TIME.get() {
        let elapsed = start.elapsed();
        let elapsed_str = format_elapsed_time(elapsed);
        println!("{} {}", msg, elapsed_str.green());
    } else {
        println!("{msg}");
    }
}

/// Event receiver that forwards ruborist events to progress bar.
pub struct ProgressReceiver;

impl utoo_ruborist::progress::EventReceiver for ProgressReceiver {
    fn on_event(&self, event: utoo_ruborist::progress::BuildEvent) {
        // Single TTY check at the receiver boundary. Non-TTY (CI, piped
        // output, `2>&1 | tee`) drops the entire indicatif update path —
        // no Mutex<ProgressState> write-lock, no format!() allocation,
        // no String clones into set_message. Worth measurable wall time
        // on the worker-pool resolve hot path; see `IS_TTY` doc comment.
        if !*IS_TTY {
            return;
        }
        use utoo_ruborist::progress::BuildEvent;
        match event {
            BuildEvent::DependencyCount { count } => {
                PROGRESS_BAR.inc_length(count as u64);
            }
            BuildEvent::Resolving { name } => {
                log_progress(&format!("resolving {}", name));
            }
            BuildEvent::Resolved { .. }
            | BuildEvent::Reused { .. }
            | BuildEvent::Skipped { .. } => {
                PROGRESS_BAR.inc(1);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_format_elapsed_time() {
        // Test hours
        let d = Duration::from_secs(3661); // 1h1m1s
        let s = format_elapsed_time(d).to_string();
        assert!(s.contains("1h1m1s"));

        // Test minutes
        let d = Duration::from_secs(125); // 2m5s
        let s = format_elapsed_time(d).to_string();
        assert!(s.contains("2m5s"));

        // Test seconds with decimal
        let d = Duration::from_millis(1100); // 1.1s
        let s = format_elapsed_time(d).to_string();
        assert_eq!(s, "[1.1s]");

        let d = Duration::from_millis(1500); // 1.5s
        let s = format_elapsed_time(d).to_string();
        assert_eq!(s, "[1.5s]");

        let d = Duration::from_millis(0); // 0.0s
        let s = format_elapsed_time(d).to_string();
        assert_eq!(s, "[0.0s]");
    }
}
