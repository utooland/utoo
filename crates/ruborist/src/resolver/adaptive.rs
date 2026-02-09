//! Additive Increase / Multiplicative Decrease concurrency controller.
//!
//! Uses latency-based congestion detection inspired by TCP Vegas/BBR:
//! - Track minimum RTT as a baseline for uncongested latency
//! - Compare each request's latency to the baseline
//! - If latency is low relative to baseline → additive increase
//! - If latency is moderately elevated → hold steady
//! - If latency spikes (> baseline × congestion factor) → multiplicative decrease
//!
//! Also handles explicit 429 rate-limit and timeout errors via `on_rate_limited()`.

/// Outcome of a fetch request, used to drive concurrency adjustments.
#[derive(Debug, Clone)]
pub enum FetchOutcome {
    /// Server returned 429 (rate limit) or request timed out
    RateLimited,
    /// Permanent error (404, parse error, etc.) — don't adjust concurrency
    PermanentError,
}

/// Classify an error for adaptive concurrency decisions.
///
/// Inspects the error message strings produced by `http.rs` to determine
/// whether the error indicates server congestion (429, timeout) or a
/// permanent issue (404, parse error).
pub fn classify_error(err: &impl std::fmt::Display) -> FetchOutcome {
    let msg = err.to_string();
    if msg.contains("HTTP 429") || msg.contains("rate limit") || msg.contains("timeout") {
        FetchOutcome::RateLimited
    } else {
        FetchOutcome::PermanentError
    }
}

const INITIAL_WINDOW: usize = 16;
const MIN_WINDOW: usize = 4;
const ADDITIVE_INCREASE: f64 = 1.0;
const DECREASE_FACTOR: f64 = 0.5;

/// Minimum floor for min_rtt to avoid false positives from cached/instant responses.
const MIN_RTT_FLOOR_MS: u64 = 50;
/// Number of samples to skip before establishing min_rtt baseline
/// (first request often includes DNS + TLS cold start).
const WARMUP_SAMPLES: usize = 2;
/// Latency ratio above which we hold steady (don't increase).
const HOLD_MULTIPLIER: f64 = 2.0;
/// Latency ratio above which we trigger multiplicative decrease.
const CONGESTION_MULTIPLIER: f64 = 4.0;

/// Adaptive concurrency controller for manifest preloading.
///
/// Uses latency-based congestion detection: tracks minimum RTT as a baseline,
/// then compares each request's latency to decide whether to increase, hold,
/// or decrease the concurrency window.
///
/// The controller is intentionally NOT async/thread-safe — it is driven
/// synchronously from the single-threaded preload loop.
#[derive(Debug)]
pub struct AdaptiveConcurrency {
    /// Current concurrency window (how many requests can be in-flight)
    current_window: usize,
    /// Minimum window (never go below this)
    min_window: usize,
    /// Maximum window (never go above this — the user-specified cap)
    max_window: usize,
    /// How much to increase per successful request
    additive_increase: f64,
    /// Factor to multiply on decrease (e.g., 0.5 = halve)
    decrease_factor: f64,
    /// Fractional accumulator for sub-integer increases
    fractional_accumulator: f64,
    /// Minimum observed RTT in ms (baseline for congestion detection).
    /// 0 means not yet established.
    min_rtt_ms: u64,
    /// Total successful samples observed (for warmup logic)
    sample_count: usize,
    /// Number of times the window was decreased
    pub decrease_count: usize,
}

impl AdaptiveConcurrency {
    /// Create a new adaptive controller with the given maximum window.
    ///
    /// Starts at `INITIAL_WINDOW` (16) and ramps up to `max_window` via
    /// additive increase on successful requests. Automatically detects
    /// congestion by tracking latency relative to the observed minimum RTT.
    pub fn new(max_window: usize) -> Self {
        Self {
            current_window: INITIAL_WINDOW.min(max_window),
            min_window: MIN_WINDOW,
            max_window,
            additive_increase: ADDITIVE_INCREASE,
            decrease_factor: DECREASE_FACTOR,
            fractional_accumulator: 0.0,
            min_rtt_ms: 0,
            sample_count: 0,
            decrease_count: 0,
        }
    }

    /// Create a fixed-window controller (no adaptation).
    ///
    /// Useful for backward compatibility and testing.
    pub fn fixed(window: usize) -> Self {
        Self {
            current_window: window,
            min_window: window,
            max_window: window,
            additive_increase: 0.0,
            decrease_factor: 1.0,
            fractional_accumulator: 0.0,
            min_rtt_ms: 0,
            sample_count: 0,
            decrease_count: 0,
        }
    }

    /// Returns the current allowed concurrency window.
    pub fn window(&self) -> usize {
        self.current_window
    }

    /// Returns the current minimum RTT baseline in ms (0 if not yet established).
    pub fn min_rtt_ms(&self) -> u64 {
        self.min_rtt_ms
    }

    /// Additive increase step (internal helper).
    fn step_increase(&mut self) {
        self.fractional_accumulator += self.additive_increase;
        if self.fractional_accumulator >= 1.0 {
            let increase = self.fractional_accumulator as usize;
            self.fractional_accumulator -= increase as f64;
            self.current_window = (self.current_window + increase).min(self.max_window);
        }
    }

    /// Multiplicative decrease step (internal helper). Returns new window.
    fn step_decrease(&mut self) -> usize {
        self.current_window =
            ((self.current_window as f64 * self.decrease_factor) as usize).max(self.min_window);
        self.fractional_accumulator = 0.0;
        self.decrease_count += 1;
        self.current_window
    }

    /// Record a successful request completion.
    ///
    /// Uses latency-based congestion detection:
    /// - `elapsed <= min_rtt × 2` → additive increase (network is healthy)
    /// - `min_rtt × 2 < elapsed <= min_rtt × 4` → hold steady (approaching capacity)
    /// - `elapsed > min_rtt × 4` → multiplicative decrease (congestion detected)
    ///
    /// During warmup (first few samples), always increases to establish baseline.
    pub fn on_success(&mut self, elapsed_ms: u64) {
        self.sample_count += 1;

        // Update min_rtt baseline (skip warmup samples for cold start)
        if self.sample_count > WARMUP_SAMPLES {
            let floored = elapsed_ms.max(MIN_RTT_FLOOR_MS);
            if self.min_rtt_ms == 0 || floored < self.min_rtt_ms {
                self.min_rtt_ms = floored;
            }
        }

        // During warmup: increase cautiously to gather baseline data
        if self.min_rtt_ms == 0 {
            self.step_increase();
            return;
        }

        let ratio = elapsed_ms as f64 / self.min_rtt_ms as f64;

        if ratio <= HOLD_MULTIPLIER {
            // Fast response relative to baseline: additive increase
            self.step_increase();
        } else if ratio <= CONGESTION_MULTIPLIER {
            // Moderately slow: hold steady, don't increase or decrease
        } else {
            // Latency spiked significantly: congestion detected
            let new_window = self.step_decrease();
            tracing::debug!(
                "Adaptive: latency congestion detected ({}ms vs min_rtt {}ms, ratio {:.1}x), window -> {}",
                elapsed_ms,
                self.min_rtt_ms,
                ratio,
                new_window,
            );
        }
    }

    /// Record a 429 rate-limit or timeout error. Returns the new window size.
    ///
    /// Multiplicative decrease: halve the window (clamped to min).
    pub fn on_rate_limited(&mut self) -> usize {
        self.step_decrease()
    }

    /// Record a permanent error (no concurrency adjustment).
    pub fn on_error(&mut self) {
        // Permanent errors (404, parse) are not congestion signals — no-op
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_window() {
        let c = AdaptiveConcurrency::new(64);
        assert_eq!(c.window(), 16);
    }

    #[test]
    fn test_initial_window_capped_by_max() {
        let c = AdaptiveConcurrency::new(8);
        assert_eq!(c.window(), 8);
    }

    #[test]
    fn test_warmup_increases() {
        let mut c = AdaptiveConcurrency::new(64);
        // During warmup (first WARMUP_SAMPLES), should increase regardless
        assert_eq!(c.window(), 16);
        c.on_success(500);
        assert_eq!(c.window(), 17);
        c.on_success(500);
        assert_eq!(c.window(), 18);
        // min_rtt not yet established
        assert_eq!(c.min_rtt_ms(), 0);
    }

    #[test]
    fn test_baseline_established_after_warmup() {
        let mut c = AdaptiveConcurrency::new(64);
        // Warmup samples
        c.on_success(200);
        c.on_success(200);
        // Third sample establishes baseline
        c.on_success(100);
        assert_eq!(c.min_rtt_ms(), 100);
    }

    #[test]
    fn test_min_rtt_floor() {
        let mut c = AdaptiveConcurrency::new(64);
        c.on_success(10);
        c.on_success(10);
        // Very fast response should be floored to MIN_RTT_FLOOR_MS
        c.on_success(10);
        assert_eq!(c.min_rtt_ms(), MIN_RTT_FLOOR_MS);
    }

    #[test]
    fn test_fast_response_increases() {
        let mut c = AdaptiveConcurrency::new(64);
        // Warmup + establish baseline at 100ms
        c.on_success(100);
        c.on_success(100);
        c.on_success(100); // establishes min_rtt = 100
        let w = c.window();
        // Response within 2x baseline → increase
        c.on_success(150);
        assert_eq!(c.window(), w + 1);
    }

    #[test]
    fn test_moderate_latency_holds() {
        let mut c = AdaptiveConcurrency::new(64);
        // Establish baseline at 100ms
        c.on_success(100);
        c.on_success(100);
        c.on_success(100);
        let w = c.window();
        // Response between 2x and 4x baseline → hold
        c.on_success(300); // 3x ratio
        assert_eq!(c.window(), w);
    }

    #[test]
    fn test_high_latency_decreases() {
        let mut c = AdaptiveConcurrency::new(64);
        // Establish baseline at 100ms
        c.on_success(100);
        c.on_success(100);
        c.on_success(100);
        let w = c.window();
        // Response > 4x baseline → decrease
        c.on_success(500); // 5x ratio
        assert_eq!(c.window(), (w as f64 * 0.5) as usize);
        assert_eq!(c.decrease_count, 1);
    }

    #[test]
    fn test_max_window_cap() {
        let mut c = AdaptiveConcurrency::new(20);
        for _ in 0..100 {
            c.on_success(50);
        }
        assert_eq!(c.window(), 20);
    }

    #[test]
    fn test_explicit_rate_limited() {
        let mut c = AdaptiveConcurrency::new(64);
        // Ramp up
        for _ in 0..16 {
            c.on_success(50);
        }
        let w = c.window();
        // Explicit 429 → halve
        c.on_rate_limited();
        assert_eq!(c.window(), (w as f64 * 0.5) as usize);
        assert_eq!(c.decrease_count, 1);
    }

    #[test]
    fn test_min_window_floor() {
        let mut c = AdaptiveConcurrency::new(64);
        for _ in 0..20 {
            c.on_rate_limited();
        }
        assert_eq!(c.window(), MIN_WINDOW);
    }

    #[test]
    fn test_fixed_mode() {
        let mut c = AdaptiveConcurrency::fixed(64);
        assert_eq!(c.window(), 64);
        c.on_success(50);
        assert_eq!(c.window(), 64);
        c.on_success(5000); // would be congestion in adaptive mode
        assert_eq!(c.window(), 64);
        c.on_rate_limited();
        assert_eq!(c.window(), 64);
    }

    #[test]
    fn test_on_error_no_change() {
        let mut c = AdaptiveConcurrency::new(64);
        let w = c.window();
        c.on_error();
        assert_eq!(c.window(), w);
    }

    #[test]
    fn test_recovery_after_congestion() {
        let mut c = AdaptiveConcurrency::new(64);
        // Establish baseline
        for _ in 0..3 {
            c.on_success(100);
        }
        // Ramp up to ~24
        for _ in 0..5 {
            c.on_success(100);
        }
        let w = c.window();
        // Congestion → decrease
        c.on_success(500);
        let decreased = c.window();
        assert!(decreased < w);
        // Recovery with fast requests
        let needed = w - decreased;
        for _ in 0..needed {
            c.on_success(100);
        }
        assert_eq!(c.window(), w);
    }

    #[test]
    fn test_min_rtt_tracks_minimum() {
        let mut c = AdaptiveConcurrency::new(64);
        c.on_success(300);
        c.on_success(300);
        c.on_success(200); // establishes min_rtt = 200
        assert_eq!(c.min_rtt_ms(), 200);
        c.on_success(100); // updates min_rtt to 100
        assert_eq!(c.min_rtt_ms(), 100);
        c.on_success(150); // doesn't change min_rtt
        assert_eq!(c.min_rtt_ms(), 100);
    }

    #[test]
    fn test_classify_429_error() {
        let err = anyhow::anyhow!("Failed to fetch lodash: HTTP 429");
        assert!(matches!(classify_error(&err), FetchOutcome::RateLimited));
    }

    #[test]
    fn test_classify_timeout_error() {
        let err = anyhow::anyhow!("Failed to fetch: Network error: timeout");
        assert!(matches!(classify_error(&err), FetchOutcome::RateLimited));
    }

    #[test]
    fn test_classify_404_error() {
        let err = anyhow::anyhow!("Package not found");
        assert!(matches!(classify_error(&err), FetchOutcome::PermanentError));
    }

    #[test]
    fn test_classify_parse_error() {
        let err = anyhow::anyhow!("JSON parse error: unexpected token");
        assert!(matches!(classify_error(&err), FetchOutcome::PermanentError));
    }
}
