use anyhow::{Context, Result};
use std::env;
use std::fs::OpenOptions;
use std::io::Write;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use indicatif::{ProgressBar, ProgressStyle};
use once_cell::sync::{Lazy, OnceCell};
use owo_colors::OwoColorize;

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

pub static PROGRESS_BAR: Lazy<ProgressBar> = Lazy::new(|| {
    let pb = ProgressBar::new(0).with_style(
        ProgressStyle::with_template("{spinner:.blue} +{pos:.green} ~{len:.magenta} {wide_msg}")
            .unwrap()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"),
    );
    pb.set_draw_target(indicatif::ProgressDrawTarget::hidden());
    pb
});

pub fn finish_progress_bar(msg: &str) {
    // If progress bar length is 0, just hide and return
    if PROGRESS_BAR.length().unwrap_or(0) == 0 {
        return;
    }
    PROGRESS_BAR.set_style(
        ProgressStyle::with_template("✓ {pos:.green}/{len:.magenta} {wide_msg}").unwrap(),
    );
    PROGRESS_BAR.finish_with_message(msg.to_string());
    PROGRESS_BAR.set_draw_target(indicatif::ProgressDrawTarget::hidden());
    // reset color
    println!("\x1b[0m");
}

pub fn abort_progress_bar() {
    PROGRESS_BAR.set_style(ProgressStyle::with_template("").unwrap());
    PROGRESS_BAR.finish_with_message("aborted".to_string());
    PROGRESS_BAR.set_draw_target(indicatif::ProgressDrawTarget::hidden());
}

pub fn start_progress_bar() {
    PROGRESS_BAR.reset();
    PROGRESS_BAR.set_style(
        ProgressStyle::with_template("{spinner:.blue} {pos:.green}/{len:.magenta} {wide_msg}")
            .unwrap()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"),
    );
    PROGRESS_BAR.set_draw_target(indicatif::ProgressDrawTarget::stderr());
    PROGRESS_BAR.enable_steady_tick(Duration::from_millis(100));
}

// add a global variable to store the verbose mode
static VERBOSE: AtomicBool = AtomicBool::new(false);

pub fn set_verbose(verbose: bool) {
    VERBOSE.store(verbose, Ordering::Relaxed);
    log_verbose("verbose mode enabled");
}

// temp log in memory
static VERBOSE_LOGS: Lazy<Mutex<Vec<String>>> = Lazy::new(|| Mutex::new(Vec::new()));

use crate::util::timer::Timer;

pub fn log_verbose(msg: &str) {
    if VERBOSE.load(Ordering::Relaxed) {
        println!("🔍 {msg}");
    }
    if let Ok(mut logs) = VERBOSE_LOGS.lock() {
        logs.push(format!("[{}][VERBOSE] {}", Timer::format_datetime(), msg));
    }
}

pub fn get_verbose_logs() -> Vec<String> {
    VERBOSE_LOGS
        .lock()
        .map(|logs| logs.clone())
        .unwrap_or_default()
}

pub fn log_warning(text: &str) {
    if VERBOSE.load(Ordering::Relaxed) {
        PROGRESS_BAR.suspend(|| println!("[WARN] {text}"));
    } else {
        PROGRESS_BAR.suspend(|| println!("{} {}", " WARN ".on_yellow(), text));
    }
    if let Ok(mut logs) = VERBOSE_LOGS.lock() {
        logs.push(format!("[{}][WARN] {}", Timer::format_datetime(), text));
    }
}

pub fn log_error(text: &str) {
    if VERBOSE.load(Ordering::Relaxed) {
        PROGRESS_BAR.suspend(|| println!("[ERROR] {text}"));
    } else {
        PROGRESS_BAR.suspend(|| println!("{} {}", " ERROR ".on_red(), text));
    }
    if let Ok(mut logs) = VERBOSE_LOGS.lock() {
        logs.push(format!("[{}][ERROR] {}", Timer::format_datetime(), text));
    }
}

pub fn log_info(text: &str) {
    if VERBOSE.load(Ordering::Relaxed) {
        PROGRESS_BAR.suspend(|| println!("[INFO] {text}"));
    }
    if let Ok(mut logs) = VERBOSE_LOGS.lock() {
        logs.push(format!("[{}][INFO] {}", Timer::format_datetime(), text));
    }
}

pub fn log_progress(text: &str) {
    PROGRESS_BAR.set_message(text.to_string());
    // log_verbose(text);
}

pub fn write_verbose_logs_to_file() -> Result<String> {
    abort_progress_bar();
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let log_file = env::temp_dir()
        .join(format!("utoo-{timestamp}.log"))
        .to_string_lossy()
        .to_string();

    let logs = get_verbose_logs();
    if !logs.is_empty() {
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&log_file)
            .context("Failed to open log file")?;

        file.write_all(logs.join("\n").as_bytes())
            .context("Failed to write logs to file")?;

        log_error(&format!("Verbose logs have been saved to {log_file}"));
    }
    Ok(log_file)
}

// Global timer for log_time/log_time_end
static START_TIME: OnceCell<Instant> = OnceCell::new();

/// Start the global timer for elapsed time logging.
pub fn log_time() {
    let _ = START_TIME.set(Instant::now());
}

/// Format a Duration into a human-readable colored string.
pub fn format_elapsed_time(elapsed: Duration) -> String {
    let total_secs = elapsed.as_secs();
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;
    // Show as x.x s if less than 60 seconds
    if hours == 0 && minutes == 0 {
        let secs = elapsed.as_secs_f64();
        // Format to 1 decimal place
        return format!("[{secs:.1}s]");
    }

    if hours > 0 {
        format!("[{hours}h{minutes}m{seconds}s]")
    } else {
        format!("[{minutes}m{seconds}s]")
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_set_verbose_true() {
        set_verbose(true);
        assert!(VERBOSE.load(Ordering::Relaxed));
    }

    #[test]
    fn test_set_verbose_false() {
        set_verbose(false);
        assert!(!VERBOSE.load(Ordering::Relaxed));
    }

    #[test]
    fn test_set_verbose_multiple_calls() {
        set_verbose(true);
        assert!(VERBOSE.load(Ordering::Relaxed));

        set_verbose(false);
        assert!(!VERBOSE.load(Ordering::Relaxed));

        set_verbose(true);
        assert!(VERBOSE.load(Ordering::Relaxed));
    }

    #[test]
    fn test_write_verbose_logs_to_file() -> Result<()> {
        set_verbose(true);
        log_verbose("Test verbose message");
        log_warning("Test warning message");
        log_error("Test error message");
        log_info("Test info message");

        let log_file = write_verbose_logs_to_file()?;
        assert!(std::path::Path::new(&log_file).exists());

        // Clean up
        std::fs::remove_file(log_file)?;
        Ok(())
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
