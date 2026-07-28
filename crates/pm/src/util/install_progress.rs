//! Shared live-progress state for the install pipeline.
//!
//! The downloader feeds a process-lifetime byte counter and an in-flight
//! download gauge, and each install records a byte baseline so the renderer
//! reports traffic for the current logical install only. Lifecycle-script queues
//! register running scripts, from wherever they run (tokio tasks, rayon
//! workers); a single render task — owned by `logger::start_progress_bar` —
//! samples this every ~120ms via [`snapshot`] and composes the spinner line with
//! [`compose_activity`] (left) and [`compose_summary`] (right). Writers never
//! touch indicatif directly, so hot paths stay lock-free and per-event
//! `set_message` contention disappears.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use colored::Colorize;
use once_cell::sync::Lazy;

/// Process-lifetime tarball bytes fetched from the network (counted as chunks
/// arrive, so retries count their real traffic).
pub static DOWNLOADED_BYTES: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy)]
pub struct DownloadBaseline(u64);

impl DownloadBaseline {
    pub fn capture() -> Self {
        Self(DOWNLOADED_BYTES.load(Ordering::Relaxed))
    }

    pub fn downloaded_bytes(self) -> u64 {
        DOWNLOADED_BYTES
            .load(Ordering::Relaxed)
            .saturating_sub(self.0)
    }
}

/// Byte baseline captured at the start of the current logical install.
static INSTALL_START_BYTES: AtomicU64 = AtomicU64::new(0);

/// Set once the install's tarball downloads are over (the clone phase has
/// finished). The later script phase then stops showing the frozen `↓ 380 MB`
/// total — by then the only network traffic is the scripts' own (e.g. puppeteer
/// fetching Chromium), which our counter doesn't track, so the figure is stale
/// and misleading.
static DOWNLOADS_DONE: AtomicBool = AtomicBool::new(false);

/// Mark the network phase finished; called once the clone phase completes.
pub fn mark_downloads_done() {
    DOWNLOADS_DONE.store(true, Ordering::Relaxed);
}

/// Start a new logical install run. The process-level byte counter keeps
/// growing, while summaries and live progress report bytes after this baseline.
pub fn start_install_run() {
    INSTALL_START_BYTES.store(DOWNLOADED_BYTES.load(Ordering::Relaxed), Ordering::Relaxed);
    DOWNLOAD_FIRST_MS.store(u64::MAX, Ordering::Relaxed);
    DOWNLOAD_LAST_MS.store(0, Ordering::Relaxed);
    DOWNLOADS_DONE.store(false, Ordering::Relaxed);
    reset_phase_state();
}

/// Bytes downloaded since [`start_install_run`] was last called.
pub fn downloaded_bytes() -> u64 {
    DOWNLOADED_BYTES
        .load(Ordering::Relaxed)
        .saturating_sub(INSTALL_START_BYTES.load(Ordering::Relaxed))
}

/// In-flight tarball downloads. Surfaced as `N downloading` so the live request
/// concurrency stays visible; unlike the per-stage extract/link gauges this one
/// holds steady at the download limit through the whole network phase.
static DOWNLOADING: AtomicUsize = AtomicUsize::new(0);

/// Monotonic baseline captured once per process. Download-window timestamps are
/// stored as millis-since-epoch in plain atomics so the hot path records the
/// active-network span lock-free.
static PROCESS_EPOCH: Lazy<Instant> = Lazy::new(Instant::now);

/// Millis-since-[`PROCESS_EPOCH`] of the first download started this run, and of
/// the last download finished. Together they bound the *network-active* window —
/// from the first tarball request to the last byte — so the summary's average
/// throughput divides by the time we were actually downloading, not the trailing
/// clone/script phases where no traffic flows. Sentinels: `u64::MAX` (no first)
/// and `0` (no last).
static DOWNLOAD_FIRST_MS: AtomicU64 = AtomicU64::new(u64::MAX);
static DOWNLOAD_LAST_MS: AtomicU64 = AtomicU64::new(0);

fn now_ms() -> u64 {
    PROCESS_EPOCH.elapsed().as_millis() as u64
}

/// Increments [`DOWNLOADING`] for the lifetime of one download; decrements on
/// drop so an early return or panic can't leak a slot. Also stamps the
/// download-window bounds: the earliest `enter` opens the window, the latest
/// `drop` closes it.
pub struct DownloadGuard;

impl DownloadGuard {
    pub fn enter() -> Self {
        DOWNLOADING.fetch_add(1, Ordering::Relaxed);
        DOWNLOAD_FIRST_MS.fetch_min(now_ms(), Ordering::Relaxed);
        Self
    }
}

impl Drop for DownloadGuard {
    fn drop(&mut self) {
        DOWNLOADING.fetch_sub(1, Ordering::Relaxed);
        DOWNLOAD_LAST_MS.fetch_max(now_ms(), Ordering::Relaxed);
    }
}

/// Duration of the network-active window for the current run — first download
/// started to last download finished — or `None` if nothing was downloaded.
/// Pairs with [`downloaded_bytes`] for an average-throughput figure that
/// excludes the no-traffic clone tail and lifecycle-script phases.
pub fn download_window() -> Option<Duration> {
    let first = DOWNLOAD_FIRST_MS.load(Ordering::Relaxed);
    let last = DOWNLOAD_LAST_MS.load(Ordering::Relaxed);
    // `>= first` (not `>`) with a 1ms floor: a fast/near-warm install whose whole
    // download span lands in one millisecond bucket still yields a throughput
    // figure, instead of inconsistently dropping `@ X/s` on small installs only.
    (first != u64::MAX && last >= first).then(|| Duration::from_millis((last - first).max(1)))
}

/// Latest one-line activity (e.g. `resolving react`, `lodash resolved`),
/// folded into the composed message when no script is running.
static ACTIVITY: Mutex<String> = Mutex::new(String::new());

pub fn set_activity(text: &str) {
    if let Ok(mut activity) = ACTIVITY.lock() {
        activity.clear();
        activity.push_str(text);
    }
}

/// A live slot for a running script's most recent output line. Silent
/// dependency scripts (e.g. `puppeteer postinstall`) capture their output, so a
/// stuck one shows nothing; the executor pushes each line in (via the closure
/// from [`ScriptGuard::sink`]) so the long-run heartbeat can surface
/// `↳ Downloading Chromium …` — what's actually taking so long. Internal: the
/// executor only ever sees the opaque sink closure, never this type.
#[derive(Clone, Default)]
struct ScriptTap(Arc<Mutex<String>>);

impl ScriptTap {
    /// Record the latest output line (already trimmed by the caller).
    fn set_line(&self, line: &str) {
        if let Ok(mut last) = self.0.lock() {
            last.clear();
            last.push_str(line);
        }
    }

    fn line(&self) -> String {
        self.0.lock().map(|l| l.clone()).unwrap_or_default()
    }
}

struct RunningScript {
    id: u64,
    label: String,
    started: Instant,
    tap: ScriptTap,
}

static RUNNING_SCRIPTS: Lazy<Mutex<Vec<RunningScript>>> = Lazy::new(|| Mutex::new(Vec::new()));
static SCRIPT_ID: AtomicU64 = AtomicU64::new(0);

/// Registers a lifecycle script as running; deregisters on drop. The renderer
/// surfaces the *longest-running* entry, so a slow `postinstall` stays visible
/// instead of being overwritten by whichever script started last.
pub struct ScriptGuard {
    id: u64,
    tap: ScriptTap,
}

impl ScriptGuard {
    /// An output sink the executor feeds this script's lines into — a plain
    /// `Fn(&str)`, so the executor stays decoupled from the progress UI.
    pub fn sink(&self) -> Arc<dyn Fn(&str) + Send + Sync> {
        let tap = self.tap.clone();
        Arc::new(move |line: &str| tap.set_line(line))
    }
}

pub fn track_script(label: String) -> ScriptGuard {
    let id = SCRIPT_ID.fetch_add(1, Ordering::Relaxed);
    let tap = ScriptTap::default();
    if let Ok(mut scripts) = RUNNING_SCRIPTS.lock() {
        scripts.push(RunningScript {
            id,
            label,
            started: Instant::now(),
            tap: tap.clone(),
        });
    }
    ScriptGuard { id, tap }
}

impl Drop for ScriptGuard {
    fn drop(&mut self) {
        if let Ok(mut scripts) = RUNNING_SCRIPTS.lock() {
            scripts.retain(|s| s.id != self.id);
        }
    }
}

/// Reset per-phase state (activity line, script registry). The byte counter and
/// install baseline are left alone so resolve-time prefetch traffic remains
/// visible during the later clone/script phases of the same install.
pub fn reset_phase_state() {
    set_activity("");
    if let Ok(mut scripts) = RUNNING_SCRIPTS.lock() {
        scripts.clear();
    }
}

/// One running lifecycle script at snapshot time.
pub struct ScriptInfo {
    /// Stable id, so the heartbeat can tell "still the same slow script" from "a
    /// new one took the lead" across render ticks.
    pub id: u64,
    pub label: String,
    pub elapsed_secs: u64,
    /// This script's most recent output line (empty if it's printed nothing).
    pub last_line: String,
}

/// Point-in-time copy of all progress state; [`compose_activity`] and
/// [`compose_summary`] render it without touching any global, so composition
/// stays a pure function.
pub struct Snapshot {
    pub bytes: u64,
    pub downloading: usize,
    /// Every running script, oldest first. The live line shows the first (the
    /// longest-running) with a `N more` tail; the heartbeat enumerates all that
    /// have crossed the slow threshold, so several stuck parallel scripts are
    /// visible at once rather than one at a time.
    pub scripts: Vec<ScriptInfo>,
    pub activity: String,
    /// Network phase concluded — suppress the cumulative `↓` total so it doesn't
    /// linger, frozen, through the script phase.
    pub downloads_done: bool,
}

pub fn snapshot() -> Snapshot {
    let scripts = RUNNING_SCRIPTS
        .lock()
        .map(|scripts| {
            let mut entries: Vec<&RunningScript> = scripts.iter().collect();
            // Oldest first: stable for the live line and gives the heartbeat a
            // deterministic order to list parallel scripts in.
            entries.sort_by_key(|s| s.started);
            entries
                .into_iter()
                .map(|s| ScriptInfo {
                    id: s.id,
                    label: s.label.clone(),
                    elapsed_secs: s.started.elapsed().as_secs(),
                    last_line: s.tap.line(),
                })
                .collect()
        })
        .unwrap_or_default();
    let activity = ACTIVITY
        .lock()
        .map(|activity| activity.clone())
        .unwrap_or_default();
    Snapshot {
        bytes: downloaded_bytes(),
        downloading: DOWNLOADING.load(Ordering::Relaxed),
        scripts,
        activity,
        downloads_done: DOWNLOADS_DONE.load(Ordering::Relaxed),
    }
}

/// Exponentially-smoothed download speed, sampled once per render tick.
/// Owns the byte-delta bookkeeping so the render loop stays a thin
/// tick → snapshot → sample → compose pipeline.
pub struct SpeedMeter {
    last_bytes: u64,
    last_tick: Instant,
    ema: f64,
}

impl SpeedMeter {
    pub fn new(bytes: u64) -> Self {
        Self::new_at(bytes, Instant::now())
    }

    fn new_at(bytes: u64, now: Instant) -> Self {
        Self {
            last_bytes: bytes,
            last_tick: now,
            ema: 0.0,
        }
    }

    /// Feed the current cumulative byte count; returns the smoothed
    /// bytes-per-second estimate.
    pub fn sample(&mut self, bytes: u64) -> f64 {
        self.sample_at(bytes, Instant::now())
    }

    fn sample_at(&mut self, bytes: u64, now: Instant) -> f64 {
        // Saturate rather than panic if a tick's clock reading is not strictly
        // monotonic; a zero dt just skips this sample's speed update.
        let dt = now.saturating_duration_since(self.last_tick).as_secs_f64();
        if dt > 0.0 {
            let instant_speed = bytes.saturating_sub(self.last_bytes) as f64 / dt;
            // EMA smooths chunk-arrival jitter without lagging far behind.
            self.ema = if self.ema == 0.0 {
                instant_speed
            } else {
                self.ema * 0.7 + instant_speed * 0.3
            };
        }
        self.last_bytes = bytes;
        self.last_tick = now;
        self.ema
    }
}

/// Once a script has run this long, its latest output line (`↳ …`) is appended
/// to the live spinner — so a slow, silent script shows *what* it's doing right
/// away, without waiting for the persistent heartbeat. Gated so fast scripts
/// (the overwhelming majority, sub-second) don't flash a tail.
const SCRIPT_TAIL_AFTER_SECS: u64 = 3;

/// The volatile left segment: what's happening *right now* — the
/// longest-running lifecycle script (with elapsed time, a `N more` tail, and —
/// once it's run a few seconds — its latest output line), or else the latest
/// activity line (`lodash resolved`). This changes once per package, so it lives
/// in the spinner's flexible `wide_msg` where it can truncate without shifting
/// the right-pinned [`compose_summary`].
pub fn compose_activity(snap: &Snapshot) -> String {
    let Some(script) = snap.scripts.first() else {
        return snap.activity.clone();
    };
    let mut part = script.label.clone();
    if script.elapsed_secs >= 1 {
        part.push_str(&format!(
            " {}",
            format!("[{}s]", script.elapsed_secs).dimmed()
        ));
    }
    let more = snap.scripts.len().saturating_sub(1);
    if more > 0 {
        part.push_str(&format!(" · {}", format!("{more} more").dimmed()));
    }
    if script.elapsed_secs >= SCRIPT_TAIL_AFTER_SECS && !script.last_line.is_empty() {
        part.push_str(&format!(" {}", format!("↳ {}", script.last_line).dimmed()));
    }
    part
}

/// The stable network column, rendered in the bar's `prefix` and pinned to a
/// fixed position just after the `pos/len` counter (the template places it
/// *before* the volatile activity). Holds cumulative throughput plus live
/// download concurrency in one place the eye can find — unlike a right-edge
/// summary, which drifts off-screen on a wide terminal.
///
/// Padded to [`SUMMARY_WIDTH`] columns so the activity text to its right keeps a
/// steady left margin. The whole column lives or dies together — present from
/// the first in-flight request (so the connect window, request open but no byte
/// yet, still reads as busy rather than idle) until the network phase ends
/// ([`Snapshot::downloads_done`]) — so its fields don't blink in and out
/// mid-phase. Cumulative bytes only grow; live request concurrency is shown as
/// `⇣N` and held *even at 0* (rather than vanishing between fetch batches) so it
/// doesn't flicker. `speed` is bytes/sec from [`SpeedMeter`], hidden below 1 B/s.
/// Empty (no padding) before anything is in flight (resolve phase / warm cache)
/// and once the network phase is over, so the script phase isn't trailed by a
/// frozen total:
///
/// `↓ 23.4 MB 8.2 MB/s · ⇣12`
pub fn compose_summary(snap: &Snapshot, speed: f64) -> String {
    // Cleared once downloads conclude, and before anything is in flight; the
    // `⇣N` indicator otherwise shows from the first request even before its
    // first byte, so a slow connect doesn't read as a hang.
    if snap.downloads_done || (snap.bytes == 0 && snap.downloading == 0) {
        return String::new();
    }

    // Build the rendered (colored) and plain forms in lockstep so the field can
    // be padded by *visible* width — owo-colors escapes would otherwise inflate
    // a byte-length count and misalign the column.
    let mut colored: Vec<String> = Vec::new();
    let mut plain: Vec<String> = Vec::new();

    if snap.bytes > 0 {
        let bytes_str = human_bytes(snap.bytes as f64);
        if speed >= 1.0 {
            let speed_str = format!("{}/s", human_bytes(speed));
            colored.push(format!("↓ {bytes_str} {}", speed_str.dimmed()));
            plain.push(format!("↓ {bytes_str} {speed_str}"));
        } else {
            colored.push(format!("↓ {bytes_str}"));
            plain.push(format!("↓ {bytes_str}"));
        }
    }

    // Live request concurrency, always shown in this phase (including 0) so it
    // holds steady between fetch batches instead of blinking off at every lull.
    let concurrency = format!("⇣{}", snap.downloading);
    colored.push(format!("{}", concurrency.dimmed()));
    plain.push(concurrency);

    let rendered = colored.join(" · ");
    let visible = plain.join(" · ").chars().count();
    match SUMMARY_WIDTH.checked_sub(visible) {
        Some(pad) if pad > 0 => format!("{rendered}{}", " ".repeat(pad)),
        _ => rendered,
    }
}

/// Fixed visible width of the [`compose_summary`] column. Sized for the *common*
/// peak — `↓ 999.9 MB 99.9 MB/s · ⇣16` (bytes, two-digit speed, two-digit
/// concurrency) — not the absolute worst case, so the activity to its right
/// isn't pushed out by a wide gutter that's almost never filled. A rare burst
/// over 100 MB/s shifts the activity by one column for that frame; not worth
/// reserving a permanent extra slot for.
pub const SUMMARY_WIDTH: usize = 26;

/// `123 B`, `45.6 kB`, `23.4 MB`, `1.2 GB` (decimal units).
pub fn human_bytes(bytes: f64) -> String {
    const UNITS: [&str; 4] = ["B", "kB", "MB", "GB"];
    let mut value = bytes;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", value as u64, UNITS[unit])
    } else {
        format!("{:.1} {}", value, UNITS[unit])
    }
}

/// Number of decimal digits in `n` (min 1) — exactly the room a counter needs
/// for its largest value, no more.
fn decimal_width(n: u64) -> usize {
    (n.max(1).ilog10() + 1) as usize
}

/// Render the `pos/len` counter with `pos` right-aligned to the digit-width of
/// `len`, so the slash holds still as `pos` climbs and the field auto-sizes to
/// each phase's magnitude — a 16-script phase gets a tight `16/16`, a 9910-pkg
/// resolve gets `9910/9910`, with no wasted padding in between. `len` is
/// monotonic within a phase, so the width only ever grows: the counter widens,
/// never shrinks mid-phase.
pub fn format_counter(pos: u64, len: u64) -> String {
    let width = decimal_width(len);
    format!(
        "{}/{}",
        format!("{pos:>width$}").green(),
        len.to_string().magenta()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serializes tests that touch process-global progress state
    /// (`DOWNLOADED_BYTES`, the install baseline, and `RUNNING_SCRIPTS`), which
    /// the default parallel test runner would otherwise race on.
    static TEST_GUARD: Mutex<()> = Mutex::new(());

    fn empty_snapshot() -> Snapshot {
        Snapshot {
            bytes: 0,
            downloading: 0,
            scripts: Vec::new(),
            activity: String::new(),
            downloads_done: false,
        }
    }

    fn script_info(label: &str, elapsed_secs: u64, last_line: &str) -> ScriptInfo {
        ScriptInfo {
            id: 0,
            label: label.to_string(),
            elapsed_secs,
            last_line: last_line.to_string(),
        }
    }

    #[test]
    fn test_human_bytes() {
        assert_eq!(human_bytes(0.0), "0 B");
        assert_eq!(human_bytes(999.0), "999 B");
        assert_eq!(human_bytes(1000.0), "1.0 kB");
        assert_eq!(human_bytes(23_400_000.0), "23.4 MB");
        assert_eq!(human_bytes(1_200_000_000.0), "1.2 GB");
    }

    #[test]
    fn test_decimal_width() {
        assert_eq!(decimal_width(0), 1);
        assert_eq!(decimal_width(9), 1);
        assert_eq!(decimal_width(10), 2);
        assert_eq!(decimal_width(16), 2);
        assert_eq!(decimal_width(999), 3);
        assert_eq!(decimal_width(5365), 4);
        assert_eq!(decimal_width(9910), 4);
    }

    #[test]
    fn test_format_counter_right_aligns_pos_to_len_width() {
        // pos right-aligned to len's width so the slash holds still as it climbs.
        assert_eq!(strip_ansi(&format_counter(0, 5365)), "   0/5365");
        assert_eq!(strip_ansi(&format_counter(4783, 5365)), "4783/5365");
        assert_eq!(strip_ansi(&format_counter(5365, 5365)), "5365/5365");
        // A small phase auto-sizes tight — no wasted padding.
        assert_eq!(strip_ansi(&format_counter(0, 16)), " 0/16");
        assert_eq!(strip_ansi(&format_counter(16, 16)), "16/16");
    }

    #[test]
    fn test_snapshot_surfaces_oldest_script() {
        let _guard = TEST_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let a = track_script("a postinstall".to_string());
        let _b = track_script("b postinstall".to_string());
        // The oldest script's output sink feeds its latest line into the snapshot.
        a.sink()("Downloading Chromium 45/120 MB");
        let scripts = snapshot().scripts;
        assert_eq!(scripts.len(), 2);
        // Oldest first.
        assert_eq!(scripts[0].label, "a postinstall");
        assert_eq!(scripts[0].last_line, "Downloading Chromium 45/120 MB");
    }

    #[test]
    fn test_compose_activity_only() {
        let mut snap = empty_snapshot();
        snap.activity = "resolving react".to_string();
        assert_eq!(compose_activity(&snap), "resolving react");
    }

    #[test]
    fn test_compose_activity_script_over_activity() {
        let mut snap = empty_snapshot();
        snap.activity = "resolving react".to_string();
        // Oldest first, plus two others → `· 2 more`.
        snap.scripts = vec![
            script_info("esbuild postinstall", 12, ""),
            script_info("b postinstall", 5, ""),
            script_info("c postinstall", 3, ""),
        ];
        let msg = compose_activity(&snap);
        assert!(msg.contains("esbuild postinstall"), "got: {msg}");
        assert!(msg.contains("[12s]"), "got: {msg}");
        assert!(msg.contains("2 more"), "got: {msg}");
        assert!(!msg.contains("resolving react"), "got: {msg}");
    }

    #[test]
    fn test_compose_activity_appends_live_tail_after_threshold() {
        let mut snap = empty_snapshot();
        snap.scripts = vec![script_info(
            "puppeteer postinstall",
            SCRIPT_TAIL_AFTER_SECS,
            "Downloading Chromium 45/120 MB",
        )];
        let msg = compose_activity(&snap);
        assert!(
            msg.contains("↳ Downloading Chromium 45/120 MB"),
            "got: {msg}"
        );

        // Below the threshold a fast script doesn't flash its output.
        snap.scripts[0].elapsed_secs = SCRIPT_TAIL_AFTER_SECS - 1;
        assert!(!compose_activity(&snap).contains("↳"), "no tail when young");

        // No output captured yet → no dangling arrow.
        snap.scripts[0].elapsed_secs = 10;
        snap.scripts[0].last_line = String::new();
        assert!(
            !compose_activity(&snap).contains("↳"),
            "no tail when silent"
        );
    }

    #[test]
    fn test_compose_summary_throughput_and_concurrency() {
        let mut snap = empty_snapshot();
        snap.bytes = 23_400_000;
        snap.downloading = 12;
        let msg = compose_summary(&snap, 8_200_000.0);
        assert!(msg.contains("↓ 23.4 MB"), "got: {msg}");
        assert!(msg.contains("8.2 MB/s"), "got: {msg}");
        assert!(msg.contains("⇣12"), "got: {msg}");
        // Empty before anything is in flight (resolve phase / warm cache) — no
        // padding so there's no blank gutter.
        assert_eq!(compose_summary(&empty_snapshot(), 0.0), "");
        // Connect window: requests in flight but no byte yet — show `⇣N` (not
        // empty) so the spinner doesn't read as idle while connecting.
        let mut connecting = empty_snapshot();
        connecting.downloading = 3;
        assert_eq!(
            strip_ansi(&compose_summary(&connecting, 0.0)).trim_end(),
            "⇣3"
        );
        // Speed drops off below 1 B/s, but the concurrency holds at `⇣0` rather
        // than vanishing — so the field doesn't blink between fetch batches.
        snap.downloading = 0;
        assert_eq!(
            strip_ansi(&compose_summary(&snap, 0.0)).trim_end(),
            "↓ 23.4 MB · ⇣0"
        );
        // Once the network phase is over, the frozen total is suppressed so the
        // script phase isn't trailed by a stale `↓` (it doesn't track the
        // scripts' own downloads).
        snap.downloads_done = true;
        assert_eq!(compose_summary(&snap, 0.0), "");
    }

    #[test]
    fn test_compose_summary_pads_to_fixed_width() {
        // A short summary is padded so the activity to its right keeps a steady
        // left margin; visible width (ANSI-stripped) lands on SUMMARY_WIDTH.
        let mut snap = empty_snapshot();
        snap.bytes = 23_400_000;
        let padded = compose_summary(&snap, 0.0);
        assert!(
            padded.ends_with(' '),
            "expected trailing pad, got: {padded:?}"
        );
        let visible = strip_ansi(&padded).chars().count();
        assert_eq!(visible, SUMMARY_WIDTH, "padded to fixed width");
    }

    /// Drop ANSI SGR escapes (`\x1b[..m`) so a test can count *visible* columns.
    fn strip_ansi(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut chars = s.chars();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                for e in chars.by_ref() {
                    if e == 'm' {
                        break;
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    #[test]
    fn test_download_window_spans_first_to_last() {
        let _guard = TEST_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        start_install_run();
        assert!(download_window().is_none(), "no downloads yet");

        let g1 = DownloadGuard::enter();
        let g2 = DownloadGuard::enter();
        drop(g1);
        // A window only exists once last > first; back-to-back guards may share a
        // millisecond, so just assert it never panics and is non-negative.
        let _window = download_window();
        drop(g2);

        // A fresh run clears the window.
        start_install_run();
        assert!(download_window().is_none(), "window reset on new run");
    }

    #[test]
    fn test_downloaded_bytes_are_per_install_run() {
        let _guard = TEST_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        DOWNLOADED_BYTES.store(1_000, Ordering::Relaxed);
        start_install_run();
        assert_eq!(downloaded_bytes(), 0);

        DOWNLOADED_BYTES.fetch_add(250, Ordering::Relaxed);
        assert_eq!(downloaded_bytes(), 250);

        start_install_run();
        assert_eq!(downloaded_bytes(), 0);
    }

    #[test]
    fn command_baseline_spans_multiple_install_runs() {
        let _guard = TEST_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        DOWNLOADED_BYTES.store(1_000, Ordering::Relaxed);
        let baseline = DownloadBaseline::capture();

        start_install_run();
        DOWNLOADED_BYTES.fetch_add(250, Ordering::Relaxed);
        start_install_run();
        DOWNLOADED_BYTES.fetch_add(400, Ordering::Relaxed);

        assert_eq!(baseline.downloaded_bytes(), 650);
        assert_eq!(downloaded_bytes(), 400);
    }

    #[test]
    fn test_speed_meter_ema() {
        let t0 = Instant::now();
        let mut meter = SpeedMeter::new_at(0, t0);
        let first = meter.sample_at(1000, t0 + Duration::from_secs(1));
        assert!((first - 1000.0).abs() < 1e-6, "got: {first}");
        // Second tick with no new bytes: 1000 * 0.7 + 0 * 0.3.
        let second = meter.sample_at(1000, t0 + Duration::from_secs(2));
        assert!((second - 700.0).abs() < 1e-6, "got: {second}");
    }
}
