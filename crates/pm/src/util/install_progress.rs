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

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;

use once_cell::sync::Lazy;
use owo_colors::OwoColorize;

/// Process-lifetime tarball bytes fetched from the network (counted as chunks
/// arrive, so retries count their real traffic).
pub static DOWNLOADED_BYTES: AtomicU64 = AtomicU64::new(0);

/// Byte baseline captured at the start of the current logical install.
static INSTALL_START_BYTES: AtomicU64 = AtomicU64::new(0);

/// Start a new logical install run. The process-level byte counter keeps
/// growing, while summaries and live progress report bytes after this baseline.
pub fn start_install_run() {
    INSTALL_START_BYTES.store(DOWNLOADED_BYTES.load(Ordering::Relaxed), Ordering::Relaxed);
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

/// Increments [`DOWNLOADING`] for the lifetime of one download; decrements on
/// drop so an early return or panic can't leak a slot.
pub struct DownloadGuard;

impl DownloadGuard {
    pub fn enter() -> Self {
        DOWNLOADING.fetch_add(1, Ordering::Relaxed);
        Self
    }
}

impl Drop for DownloadGuard {
    fn drop(&mut self) {
        DOWNLOADING.fetch_sub(1, Ordering::Relaxed);
    }
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

struct RunningScript {
    id: u64,
    label: String,
    started: Instant,
}

static RUNNING_SCRIPTS: Lazy<Mutex<Vec<RunningScript>>> = Lazy::new(|| Mutex::new(Vec::new()));
static SCRIPT_ID: AtomicU64 = AtomicU64::new(0);

/// Registers a lifecycle script as running; deregisters on drop. The renderer
/// surfaces the *longest-running* entry, so a slow `postinstall` stays visible
/// instead of being overwritten by whichever script started last.
pub struct ScriptGuard(u64);

pub fn track_script(label: String) -> ScriptGuard {
    let id = SCRIPT_ID.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut scripts) = RUNNING_SCRIPTS.lock() {
        scripts.push(RunningScript {
            id,
            label,
            started: Instant::now(),
        });
    }
    ScriptGuard(id)
}

impl Drop for ScriptGuard {
    fn drop(&mut self) {
        if let Ok(mut scripts) = RUNNING_SCRIPTS.lock() {
            scripts.retain(|s| s.id != self.0);
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

/// The longest-running script at snapshot time.
pub struct ScriptDisplay {
    pub label: String,
    pub elapsed_secs: u64,
    /// How many other scripts are running besides this one.
    pub more: usize,
}

/// Point-in-time copy of all progress state; [`compose_activity`] and
/// [`compose_summary`] render it without touching any global, so composition
/// stays a pure function.
pub struct Snapshot {
    pub bytes: u64,
    pub downloading: usize,
    pub script: Option<ScriptDisplay>,
    pub activity: String,
}

pub fn snapshot() -> Snapshot {
    let script = RUNNING_SCRIPTS.lock().ok().and_then(|scripts| {
        let oldest = scripts.iter().min_by_key(|s| s.started)?;
        Some(ScriptDisplay {
            label: oldest.label.clone(),
            elapsed_secs: oldest.started.elapsed().as_secs(),
            more: scripts.len() - 1,
        })
    });
    let activity = ACTIVITY
        .lock()
        .map(|activity| activity.clone())
        .unwrap_or_default();
    Snapshot {
        bytes: downloaded_bytes(),
        downloading: DOWNLOADING.load(Ordering::Relaxed),
        script,
        activity,
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
        let dt = now.duration_since(self.last_tick).as_secs_f64();
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

/// The volatile left segment: what's happening *right now* — the
/// longest-running lifecycle script (with elapsed time and a `N more` tail), or
/// else the latest activity line (`lodash resolved`). This changes once per
/// package, so it lives in the spinner's flexible `wide_msg` where it can
/// truncate without shifting the right-pinned [`compose_summary`].
pub fn compose_activity(snap: &Snapshot) -> String {
    let Some(script) = &snap.script else {
        return snap.activity.clone();
    };
    let mut part = script.label.clone();
    if script.elapsed_secs >= 1 {
        part.push_str(&format!(
            " {}",
            format!("[{}s]", script.elapsed_secs).dimmed()
        ));
    }
    if script.more > 0 {
        part.push_str(&format!(" · {}", format!("{} more", script.more).dimmed()));
    }
    part
}

/// The stable right segment, pinned to the right edge (via the bar's `prefix`)
/// so it holds a steady position while the activity churns: cumulative network
/// throughput plus live download concurrency.
///
/// Cumulative bytes only grow and the download count holds at the limit through
/// the network phase, so this stays put — deliberately *not* the extract/link
/// gauges, which blink on/off as worker batches drain. `speed` is bytes/sec
/// from [`SpeedMeter`], hidden below 1 B/s so it drops off cleanly once
/// downloads finish. Empty before any byte arrives and no download is in flight
/// (e.g. the resolve phase, or a fully warm cache):
///
/// `↓ 23.4 MB 8.2 MB/s · 12 downloading`
pub fn compose_summary(snap: &Snapshot, speed: f64) -> String {
    let mut parts: Vec<String> = Vec::new();

    if snap.bytes > 0 {
        let mut net = format!("↓ {}", human_bytes(snap.bytes as f64));
        if speed >= 1.0 {
            net.push_str(&format!(
                " {}",
                format!("{}/s", human_bytes(speed)).dimmed()
            ));
        }
        parts.push(net);
    }

    if snap.downloading > 0 {
        parts.push(format!(
            "{}",
            format!("{} downloading", snap.downloading).dimmed()
        ));
    }

    parts.join(" · ")
}

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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn empty_snapshot() -> Snapshot {
        Snapshot {
            bytes: 0,
            downloading: 0,
            script: None,
            activity: String::new(),
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
    fn test_snapshot_surfaces_oldest_script() {
        let _a = track_script("a postinstall".to_string());
        let _b = track_script("b postinstall".to_string());
        let script = snapshot().script.expect("scripts registered");
        assert_eq!(script.label, "a postinstall");
        assert_eq!(script.more, 1);
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
        snap.script = Some(ScriptDisplay {
            label: "esbuild postinstall".to_string(),
            elapsed_secs: 12,
            more: 2,
        });
        let msg = compose_activity(&snap);
        assert!(msg.contains("esbuild postinstall"), "got: {msg}");
        assert!(msg.contains("[12s]"), "got: {msg}");
        assert!(msg.contains("2 more"), "got: {msg}");
        assert!(!msg.contains("resolving react"), "got: {msg}");
    }

    #[test]
    fn test_compose_summary_throughput_and_concurrency() {
        let mut snap = empty_snapshot();
        snap.bytes = 23_400_000;
        snap.downloading = 12;
        let msg = compose_summary(&snap, 8_200_000.0);
        assert!(msg.contains("↓ 23.4 MB"), "got: {msg}");
        assert!(msg.contains("8.2 MB/s"), "got: {msg}");
        assert!(msg.contains("12 downloading"), "got: {msg}");
        // Empty before any byte arrives and nothing is downloading (resolve
        // phase / warm cache).
        assert_eq!(compose_summary(&empty_snapshot(), 0.0), "");
        // Speed drops off below 1 B/s; download count drops off once drained.
        snap.downloading = 0;
        assert_eq!(compose_summary(&snap, 0.0), "↓ 23.4 MB");
    }

    #[test]
    fn test_downloaded_bytes_are_per_install_run() {
        DOWNLOADED_BYTES.store(1_000, Ordering::Relaxed);
        start_install_run();
        assert_eq!(downloaded_bytes(), 0);

        DOWNLOADED_BYTES.fetch_add(250, Ordering::Relaxed);
        assert_eq!(downloaded_bytes(), 250);

        start_install_run();
        assert_eq!(downloaded_bytes(), 0);
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
