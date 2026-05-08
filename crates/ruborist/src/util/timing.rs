//! Per-phase manifest fetch timing accumulator for p1 perf investigation.
//!
//! Splits each `fetch_*_manifest` call into three observable pieces:
//!   - `request_us`: from `request.send().await` to response headers
//!     received. Captures TCP connect (when not pooled), TLS handshake,
//!     HTTP request roundtrip, and server-side processing.
//!   - `body_us`: from response headers to the entire JSON body buffered.
//!     Network-bandwidth bound for large packuments.
//!   - `parse_us`: from full body buffered to a typed manifest. CPU bound
//!     (simd_json on a spawn_blocking thread).
//!
//! `parse_us` is wall-clock for the await on `parse_json_off_runtime` —
//! since JSON parse runs on `spawn_blocking`, this includes scheduling
//! latency rather than pure CPU time. Together with the per-fetch total
//! already tracked in `preload_manifests`, this lets us answer "where
//! did p1's wall time go?" without external profiling.
//!
//! All counters are `AtomicU64` so the recording path is lock-free.
//! Numbers are reset between resolves via [`reset()`] so successive
//! `utoo deps` invocations report independently.

use std::sync::atomic::{AtomicU64, Ordering};

/// Per-process accumulator for manifest fetch timings.
#[derive(Default, Debug)]
pub struct FetchTimings {
    /// Number of fetches recorded (full + version manifest).
    pub count: AtomicU64,
    /// Sum of microseconds spent in `request.send().await`.
    pub request_us: AtomicU64,
    /// Sum of microseconds spent in `response.bytes().await`.
    pub body_us: AtomicU64,
    /// Sum of microseconds spent awaiting `parse_json_off_runtime`.
    pub parse_us: AtomicU64,
    /// Sum of body bytes received across all fetches.
    pub bytes: AtomicU64,
}

impl FetchTimings {
    /// Record one fetch's split timings. Call once per successful fetch.
    pub fn record(&self, request_us: u64, body_us: u64, parse_us: u64, bytes: u64) {
        self.count.fetch_add(1, Ordering::Relaxed);
        self.request_us.fetch_add(request_us, Ordering::Relaxed);
        self.body_us.fetch_add(body_us, Ordering::Relaxed);
        self.parse_us.fetch_add(parse_us, Ordering::Relaxed);
        self.bytes.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Reset all counters to zero.
    pub fn reset(&self) {
        self.count.store(0, Ordering::Relaxed);
        self.request_us.store(0, Ordering::Relaxed);
        self.body_us.store(0, Ordering::Relaxed);
        self.parse_us.store(0, Ordering::Relaxed);
        self.bytes.store(0, Ordering::Relaxed);
    }

    /// Snapshot of the current accumulator state.
    pub fn snapshot(&self) -> FetchTimingsSnapshot {
        FetchTimingsSnapshot {
            count: self.count.load(Ordering::Relaxed),
            request_us: self.request_us.load(Ordering::Relaxed),
            body_us: self.body_us.load(Ordering::Relaxed),
            parse_us: self.parse_us.load(Ordering::Relaxed),
            bytes: self.bytes.load(Ordering::Relaxed),
        }
    }
}

/// Immutable snapshot suitable for printing.
#[derive(Debug, Clone, Copy)]
pub struct FetchTimingsSnapshot {
    pub count: u64,
    pub request_us: u64,
    pub body_us: u64,
    pub parse_us: u64,
    pub bytes: u64,
}

impl FetchTimingsSnapshot {
    /// One-line summary for tracing logs.
    pub fn summary_line(&self) -> String {
        if self.count == 0 {
            return "fetch-timings: no requests recorded".to_string();
        }
        let count = self.count;
        let avg_req = self.request_us / count;
        let avg_body = self.body_us / count;
        let avg_parse = self.parse_us / count;
        let avg_bytes = self.bytes / count;
        format!(
            "fetch-timings: n={} sum_request={}ms sum_body={}ms sum_parse={}ms total_bytes={}MB | avg_request={}us avg_body={}us avg_parse={}us avg_bytes={}KB",
            count,
            self.request_us / 1_000,
            self.body_us / 1_000,
            self.parse_us / 1_000,
            self.bytes / 1_000_000,
            avg_req,
            avg_body,
            avg_parse,
            avg_bytes / 1_024,
        )
    }
}

/// Process-wide manifest fetch timing accumulator.
pub static FETCH_TIMINGS: FetchTimings = FetchTimings {
    count: AtomicU64::new(0),
    request_us: AtomicU64::new(0),
    body_us: AtomicU64::new(0),
    parse_us: AtomicU64::new(0),
    bytes: AtomicU64::new(0),
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_and_snapshot() {
        FETCH_TIMINGS.reset();
        FETCH_TIMINGS.record(100, 200, 300, 1024);
        FETCH_TIMINGS.record(150, 250, 350, 2048);
        let snap = FETCH_TIMINGS.snapshot();
        assert_eq!(snap.count, 2);
        assert_eq!(snap.request_us, 250);
        assert_eq!(snap.body_us, 450);
        assert_eq!(snap.parse_us, 650);
        assert_eq!(snap.bytes, 3072);
        FETCH_TIMINGS.reset();
        let snap2 = FETCH_TIMINGS.snapshot();
        assert_eq!(snap2.count, 0);
    }
}
