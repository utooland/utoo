use anyhow::Result;
use std::env;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::util::format_print::{HeartbeatScript, print_script_heartbeat};
use crate::util::install_progress;
use crate::util::invocation;

use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use once_cell::sync::{Lazy, OnceCell};

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{
    EnvFilter, Layer, Registry, fmt, layer::SubscriberExt, util::SubscriberInitExt,
};
use utoo_ruborist::progress::BuildEvent;

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

/// The bar's spinner-line templates. `with_template` only fails on a malformed
/// template *string*; these are compile-time constants, so the `.unwrap()`s at
/// the use sites assert "this literal is well-formed" — a programmer error, not
/// a runtime one. They're pulled out as consts so `test_progress_templates_parse`
/// can verify them in CI, turning a would-be runtime panic (rendering is gated
/// behind a TTY, which CI isn't) into a test failure.
const TEMPLATE_INIT: &str = "{spinner:.blue} +{pos:.green} ~{len:.magenta} {wide_msg}";
const TEMPLATE_RUNNING: &str = "{spinner:.blue} {prefix} {wide_msg}";
const TEMPLATE_FINISH: &str = "✓ {wide_msg}";
const TICK_CHARS: &str = "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏";

pub static PROGRESS_BAR: Lazy<ProgressBar> = Lazy::new(|| {
    let pb = ProgressBar::new(0).with_style(
        ProgressStyle::with_template(TEMPLATE_INIT)
            .unwrap()
            .tick_chars(TICK_CHARS),
    );
    pb.set_draw_target(indicatif::ProgressDrawTarget::hidden());
    pb
});

/// Console writer for the tracing fmt layer that cooperates with the spinner.
///
/// The bar redraws in place on stderr; a raw stdout write while it is active
/// lands at the cursor position and glues the log line onto a stale bar
/// frame. Buffering the event and flushing inside `suspend` lets indicatif
/// clear the bar, print the line, and redraw below it. With the bar hidden
/// (non-TTY, or outside a progress phase) `suspend` just runs the closure.
struct ProgressConsoleWriter(Vec<u8>);

impl std::io::Write for ProgressConsoleWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Drop for ProgressConsoleWriter {
    fn drop(&mut self) {
        if self.0.is_empty() {
            return;
        }
        PROGRESS_BAR.suspend(|| {
            use std::io::Write;
            let _ = std::io::stderr().write_all(&self.0);
        });
    }
}

// Global state for tracing
static LOG_FILE_PATH: OnceCell<PathBuf> = OnceCell::new();

/// Initialize tracing subscriber with console and file output
/// Returns (log_path, guard) - the guard must be kept alive for the duration of the program
pub fn init_tracing(verbose: bool, quiet: bool) -> Result<(PathBuf, WorkerGuard)> {
    // 1. Build environment filters
    // Note: Binary name is "utoo", so module paths start with "utoo::" not "utoo_pm::"

    // Console filter: verbose mode shows debug, otherwise show info+
    let console_level = if quiet {
        "off"
    } else if verbose {
        "debug"
    } else {
        "info"
    };
    let console_filter = if quiet {
        EnvFilter::new("off")
    } else {
        EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new(format!("utoo={console_level}")))
    };

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
    let is_tty = std::io::stderr().is_terminal();

    // 4. Build subscriber with different filters for console and file
    Registry::default()
        .with(
            fmt::layer()
                .with_writer(|| ProgressConsoleWriter(Vec::new()))
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

/// Write a line to stdout, swallowing `BrokenPipe` (and any other write
/// failure) so a closed downstream reader never aborts the process.
///
/// We keep SIGPIPE ignored (see [`crate::util::sysconf`]) so install work
/// survives a broken stdout; the trailing summary lines printed here must
/// degrade just as quietly. Plain `println!` would instead panic with
/// "failed printing to stdout: Broken pipe" at the finish line.
fn println_lossy(args: std::fmt::Arguments<'_>) {
    if invocation::json() || invocation::quiet() {
        return;
    }
    use std::io::Write;
    // Lock once so the line and its newline are written under a single lock
    // (no interleaving with other writers), and stream `args` straight to the
    // writer instead of re-wrapping it in another format pass.
    let mut out = std::io::stdout().lock();
    let _ = out.write_fmt(args).and_then(|_| out.write_all(b"\n"));
}

/// Finish the progress bar, optionally appending a dimmed `[2.6s]` suffix.
pub fn finish_progress_bar(msg: &str, elapsed: Option<Duration>) {
    // Stop the render task first so it can't overwrite the final message.
    stop_render_task();
    if PROGRESS_BAR.length().unwrap_or(0) == 0 {
        return;
    }
    // Render the counter into the message (same auto-width as the running line)
    // so the figures don't jump when the spinner is replaced by the final ✓ line.
    let counter = install_progress::format_counter(
        PROGRESS_BAR.position(),
        PROGRESS_BAR.length().unwrap_or(0),
    );
    PROGRESS_BAR.set_style(ProgressStyle::with_template(TEMPLATE_FINISH).unwrap());
    let full_msg = match elapsed {
        Some(d) => format!("{counter} {msg} {}", format_elapsed_time(d).dimmed()),
        None => format!("{counter} {msg}"),
    };
    PROGRESS_BAR.finish_with_message(full_msg);
    PROGRESS_BAR.set_draw_target(indicatif::ProgressDrawTarget::hidden());
    // reset color
    println_lossy(format_args!("\x1b[0m"));
}

/// Print the install counts line, e.g.
/// `+ 513 added · 3017 reused · 123 downloaded`.
///
/// Semantics match pnpm:
/// - `added`: packages linked into `node_modules/` this run
/// - `reused`: tarballs served from the local cache (no network)
/// - `downloaded`: tarballs fetched from the registry
pub fn print_install_counts(added: usize, reused: usize, downloaded: usize) {
    let bytes = install_progress::downloaded_bytes();
    let traffic = if bytes > 0 {
        // Average over the *network-active* window only — first download started
        // to last byte received — so the figure reflects real link throughput.
        // Dividing by the whole run would fold in the trailing no-traffic clone
        // and lifecycle-script phases and badly understate the speed.
        let avg = install_progress::download_window()
            .map(|d| d.as_secs_f64())
            .filter(|s| *s > 0.0)
            .map(|s| format!(" @ {}/s", install_progress::human_bytes(bytes as f64 / s)))
            .unwrap_or_default();
        format!(" ({}{})", install_progress::human_bytes(bytes as f64), avg)
    } else {
        String::new()
    };
    println_lossy(format_args!(
        "+ {} {} · {} {} · {} {}{}",
        added.to_string().green(),
        "added".dimmed(),
        reused.to_string().magenta(),
        "reused".dimmed(),
        downloaded.to_string().cyan(),
        "downloaded".dimmed(),
        traffic.dimmed(),
    ));
}

/// Render task started by `start_progress_bar`, stopped by
/// `finish_progress_bar`. It is the only writer of the spinner *message*:
/// every ~120ms it samples `install_progress` state (bytes, stage gauges,
/// running scripts, latest activity) and composes one line, so hot paths only
/// touch atomics and never contend on indicatif's internal mutex.
static RENDER_TASK: Mutex<Option<tokio::task::JoinHandle<()>>> = Mutex::new(None);

fn spawn_render_task() -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(120));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut meter = install_progress::SpeedMeter::new(install_progress::downloaded_bytes());
        let mut heartbeat = ScriptHeartbeat::default();
        loop {
            interval.tick().await;
            let snap = install_progress::snapshot();
            let speed = meter.sample(snap.bytes);
            // Volatile activity goes in the flexible wide message; the counter
            // and fixed-width network summary share the prefix, pinned just after
            // the spinner so they stay in a stable, readable spot while the
            // activity churns.
            let counter = install_progress::format_counter(
                PROGRESS_BAR.position(),
                PROGRESS_BAR.length().unwrap_or(0),
            );
            let summary = install_progress::compose_summary(&snap, speed);
            let prefix = if summary.is_empty() {
                counter
            } else {
                format!("{counter} {summary}")
            };
            PROGRESS_BAR.set_message(install_progress::compose_activity(&snap));
            PROGRESS_BAR.set_prefix(prefix);
            heartbeat.maybe_emit(&snap);
        }
    })
}

/// How long a silent dependency script must run before its *first* persistent
/// heartbeat. Set high: this line is `println`'d into the scrollback and can't
/// be unprinted, so a script that finishes a few seconds later would leave a
/// misleading "still running" record. Fast visibility is already covered by the
/// live spinner tail (`SCRIPT_TAIL_AFTER_SECS`), which clears when the script
/// ends; the persistent record is reserved for genuinely stuck tasks, where a
/// scroll-up note earns its keep.
const SCRIPT_HEARTBEAT_FIRST_SECS: u64 = 30;

/// Gap between repeat heartbeats after the first — sparse, since each is a
/// permanent block; the `↳` output is refreshed each time so a long download
/// still shows progress.
const SCRIPT_HEARTBEAT_REPEAT_SECS: u64 = 30;

/// Per-render-task state that throttles the long-running-script heartbeat,
/// resetting when a different script becomes the longest-running one.
#[derive(Default)]
struct ScriptHeartbeat {
    /// Id of the script the cadence is keyed to (`None` = none yet).
    id: Option<u64>,
    /// Elapsed seconds at the last heartbeat for that script (`None` = not yet
    /// fired), so the first fires at [`SCRIPT_HEARTBEAT_FIRST_SECS`] and repeats
    /// every [`SCRIPT_HEARTBEAT_REPEAT_SECS`] after.
    last_fired_secs: Option<u64>,
}

impl ScriptHeartbeat {
    fn maybe_emit(&mut self, snap: &install_progress::Snapshot) {
        // The longest-running script (first, oldest) drives the cadence.
        let Some(oldest) = snap.scripts.first() else {
            *self = Self::default();
            return;
        };
        if self.id != Some(oldest.id) {
            self.id = Some(oldest.id);
            self.last_fired_secs = None;
        }
        let elapsed = oldest.elapsed_secs;
        let due = match self.last_fired_secs {
            None => elapsed >= SCRIPT_HEARTBEAT_FIRST_SECS,
            Some(last) => elapsed >= last + SCRIPT_HEARTBEAT_REPEAT_SECS,
        };
        if !due {
            return;
        }

        // List *every* script past the first-trigger threshold, so several
        // parallel scripts stuck at once are all visible — not surfaced one at a
        // time as each finishes.
        let slow: Vec<HeartbeatScript> = snap
            .scripts
            .iter()
            .filter(|s| s.elapsed_secs >= SCRIPT_HEARTBEAT_FIRST_SECS)
            .map(|s| HeartbeatScript {
                label: &s.label,
                secs: s.elapsed_secs,
                last_line: &s.last_line,
            })
            .collect();
        if slow.is_empty() {
            return;
        }
        self.last_fired_secs = Some(elapsed);
        let log = get_log_file_path().map(|p| p.as_path());
        // `suspend` clears the live bar, prints above it, and redraws — the only
        // safe way to emit a persistent line while the spinner runs.
        PROGRESS_BAR.suspend(|| {
            print_script_heartbeat(&slow, log);
        });
    }
}

fn stop_render_task() {
    if let Ok(mut task) = RENDER_TASK.lock()
        && let Some(task) = task.take()
    {
        task.abort();
    }
}

pub fn start_progress_bar() {
    if !*IS_TTY || invocation::json() || invocation::quiet() {
        return;
    }
    install_progress::reset_phase_state();
    PROGRESS_BAR.reset();
    // Clear the previous phase's finish message/summary so neither lingers on
    // the fresh bar until the render task's first tick.
    PROGRESS_BAR.set_message("");
    PROGRESS_BAR.set_prefix("");
    // The counter and network summary both live in `{prefix}`, which the render
    // task rebuilds each tick (see `format_counter`): indicatif can't size a
    // template field to a value it doesn't know yet, and the counter width has to
    // track `len`, which only becomes known — and keeps growing — during resolve.
    // `{prefix}` is pinned just after the spinner so it stays in the eye's path
    // on a wide terminal; `{wide_msg}` (volatile activity) fills the rest.
    PROGRESS_BAR.set_style(
        ProgressStyle::with_template(TEMPLATE_RUNNING)
            .unwrap()
            .tick_chars(TICK_CHARS),
    );
    PROGRESS_BAR.set_draw_target(indicatif::ProgressDrawTarget::stderr());
    PROGRESS_BAR.enable_steady_tick(Duration::from_millis(100));
    if let Ok(mut task) = RENDER_TASK.lock()
        && task.is_none()
    {
        *task = Some(spawn_render_task());
    }
}

pub fn log_progress(text: &str) {
    if !*IS_TTY {
        return;
    }
    // The render task folds this into the composed message on its next tick.
    install_progress::set_activity(text);
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
    println_lossy(format_args!(""));
    if let Some(start) = START_TIME.get() {
        let elapsed = start.elapsed();
        let elapsed_str = format_elapsed_time(elapsed);
        println_lossy(format_args!("{} {}", msg, elapsed_str.green()));
    } else {
        println_lossy(format_args!("{msg}"));
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
    fn test_progress_templates_parse() {
        // The use sites `.unwrap()` these; rendering is TTY-gated so a malformed
        // template would only panic on a real terminal, never in CI. Parse them
        // here so a bad edit fails the test suite instead.
        for template in [TEMPLATE_INIT, TEMPLATE_RUNNING, TEMPLATE_FINISH] {
            ProgressStyle::with_template(template)
                .unwrap_or_else(|e| panic!("malformed progress template {template:?}: {e}"));
        }
    }

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
