//! Frame-time metrics for display-link-driven UIs.
//!
//! A platform's display-link driver (CADisplayLink on Apple, Choreographer on
//! Android, `requestAnimationFrame` in WASM) calls
//! [`js_frame_metrics_record`] once per vsync with the vsync timestamp and the
//! display's *nominal* frame duration. This module turns that stream into the
//! distribution numbers a UI benchmark actually needs — p50/p95/p99 frame
//! times and a dropped-frame count — rather than an average FPS, which hides
//! exactly the stutter people care about.
//!
//! # Why this does not reuse `js_timer_now`
//!
//! [`crate::timer::js_timer_now`] truncates to whole milliseconds
//! (`as_millis() as f64`). At a 120 Hz frame budget of 8.33 ms that quantizes
//! every sample to ~12% of the budget, which cannot produce a meaningful p99.
//! Drivers must pass a high-resolution timestamp from their own clock
//! (`CADisplayLink.timestamp` is a `CFTimeInterval`, i.e. sub-microsecond).
//!
//! # Dropped frames
//!
//! A frame is dropped when the interval to the previous vsync spans more than
//! one nominal frame. `dropped = round(interval / expected) - 1`, so an
//! interval within half a frame of nominal counts as zero and a doubled
//! interval counts as one. Deriving this from the driver-supplied `expected`
//! rather than a hardcoded 60 Hz is what makes the same code correct on a
//! 120 Hz ProMotion display.
//!
//! # Discontinuities
//!
//! Backgrounding an app pauses its display link. The gap that follows is not a
//! 4000 ms frame — attributing it as one would report hundreds of phantom
//! dropped frames and poison the p99. Drivers must call
//! [`js_frame_metrics_mark_discontinuity`] around any deliberate pause so the
//! next vsync starts a fresh interval.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

/// Retained frame-interval samples. 8192 covers ~68 s at 120 Hz / ~136 s at
/// 60 Hz; past that the oldest samples are overwritten. Percentiles are over
/// the retained window, which is what you want for a bounded-memory probe
/// running inside a shipping app.
const CAPACITY: usize = 8192;

/// An interval this long is a stall, not a frame — but it is still recorded,
/// because a real 500 ms hitch is exactly what a benchmark is hunting. Only
/// the *dropped-frame* attribution is capped, so one hitch cannot dominate the
/// count. Deliberate pauses should use `mark_discontinuity` instead.
const MAX_DROPPED_ATTRIBUTION: u64 = 64;

struct FrameMetrics {
    /// Ring of frame intervals in milliseconds.
    samples: Vec<f64>,
    next: usize,
    filled: bool,
    last_ts_ms: Option<f64>,
    total_frames: u64,
    dropped_frames: u64,
    longest_ms: f64,
    /// Most recent nominal frame duration reported by the driver.
    expected_ms: f64,
    /// Frames recorded since the last automatic report.
    frames_since_report: u64,
}

impl FrameMetrics {
    const fn new() -> Self {
        Self {
            samples: Vec::new(),
            next: 0,
            filled: false,
            last_ts_ms: None,
            total_frames: 0,
            dropped_frames: 0,
            longest_ms: 0.0,
            expected_ms: 0.0,
            frames_since_report: 0,
        }
    }

    fn reset(&mut self) {
        self.reset_window();
        self.last_ts_ms = None;
        self.expected_ms = 0.0;
    }

    /// Clear the sample window and counters but keep the interval baseline and
    /// the known frame budget.
    ///
    /// Used between automatic reports so each printed line describes a distinct
    /// interval. Preserving `last_ts_ms` matters: clearing it would drop the
    /// frame straddling the boundary, so a run reporting every N frames would
    /// silently lose one interval per report.
    fn reset_window(&mut self) {
        self.samples.clear();
        self.next = 0;
        self.filled = false;
        self.total_frames = 0;
        self.dropped_frames = 0;
        self.longest_ms = 0.0;
        self.frames_since_report = 0;
    }

    fn push_sample(&mut self, interval_ms: f64) {
        if self.samples.len() < CAPACITY {
            self.samples.push(interval_ms);
            self.next = self.samples.len() % CAPACITY;
            if self.samples.len() == CAPACITY {
                self.filled = true;
            }
        } else {
            self.samples[self.next] = interval_ms;
            self.next = (self.next + 1) % CAPACITY;
            self.filled = true;
        }
    }

    fn record(&mut self, timestamp_ms: f64, expected_frame_ms: f64) {
        if !timestamp_ms.is_finite() {
            return;
        }
        if expected_frame_ms.is_finite() && expected_frame_ms > 0.0 {
            self.expected_ms = expected_frame_ms;
        }

        let previous = self.last_ts_ms.replace(timestamp_ms);
        let Some(previous) = previous else {
            // First vsync after start or after a discontinuity: establishes the
            // baseline, but there is no interval to attribute yet.
            return;
        };
        let interval_ms = timestamp_ms - previous;
        // A non-monotonic driver timestamp is a driver bug, not a 0 ms frame.
        // Drop the sample and re-baseline rather than recording a negative.
        if interval_ms <= 0.0 {
            return;
        }

        self.total_frames += 1;
        self.frames_since_report += 1;
        self.push_sample(interval_ms);
        if interval_ms > self.longest_ms {
            self.longest_ms = interval_ms;
        }

        if self.expected_ms > 0.0 {
            let spans = (interval_ms / self.expected_ms).round();
            if spans > 1.0 {
                let dropped = (spans as u64).saturating_sub(1);
                self.dropped_frames += dropped.min(MAX_DROPPED_ATTRIBUTION);
            }
        }
    }

    fn sorted_samples(&self) -> Vec<f64> {
        let mut out = self.samples.clone();
        out.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        out
    }

    /// One-line summary of the current window, or `None` if it holds no frames.
    fn summary_line(&self) -> Option<String> {
        let sorted = self.sorted_samples();
        if sorted.is_empty() {
            return None;
        }
        Some(format!(
            "[frame-stats] frames={} budget={:.2}ms p50={:.2}ms p95={:.2}ms p99={:.2}ms max={:.2}ms dropped={}",
            self.total_frames,
            self.expected_ms,
            percentile_of(&sorted, 50.0),
            percentile_of(&sorted, 95.0),
            percentile_of(&sorted, 99.0),
            self.longest_ms,
            self.dropped_frames,
        ))
    }
}

/// Nearest-rank percentile. `p` is in `[0, 100]`.
///
/// Nearest-rank (rather than an interpolating definition) means every reported
/// value is a frame interval that actually occurred, which is the right
/// property for a latency distribution.
fn percentile_of(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let p = p.clamp(0.0, 100.0);
    let rank = (p / 100.0 * sorted.len() as f64).ceil() as usize;
    let idx = rank.saturating_sub(1).min(sorted.len() - 1);
    sorted[idx]
}

static METRICS: Mutex<FrameMetrics> = Mutex::new(FrameMetrics::new());

fn env_default_enabled() -> bool {
    match std::env::var("PERRY_FRAME_STATS") {
        Ok(v) => !matches!(v.as_str(), "" | "0" | "off" | "false"),
        Err(_) => false,
    }
}

fn enabled_flag() -> &'static AtomicBool {
    static FLAG: OnceLock<AtomicBool> = OnceLock::new();
    FLAG.get_or_init(|| AtomicBool::new(env_default_enabled()))
}

/// Whether frame metrics are being collected.
///
/// Defaults to the `PERRY_FRAME_STATS` env var and can be flipped at runtime
/// via [`js_frame_metrics_set_enabled`], so a benchmark app can scope
/// collection to one workload without relaunching under a different
/// environment.
pub fn frame_metrics_enabled() -> bool {
    enabled_flag().load(Ordering::Relaxed)
}

/// C-ABI view of [`frame_metrics_enabled`].
///
/// Platform UI crates must reach the runtime through the C ABI rather than as
/// a Rust path — see the note in `perry-ui-ios/src/frame_driver.rs` — so the
/// predicate needs an `extern "C"` form.
#[no_mangle]
pub extern "C" fn js_frame_metrics_enabled() -> i32 {
    if frame_metrics_enabled() {
        1
    } else {
        0
    }
}

/// Enable (`1`) or disable (`0`) collection. Disabling does not clear samples
/// already collected; pair with [`js_frame_metrics_reset`] to start clean.
#[no_mangle]
pub extern "C" fn js_frame_metrics_set_enabled(on: i32) {
    enabled_flag().store(on != 0, Ordering::Relaxed);
}

/// Record one vsync.
///
/// `timestamp_ms` must come from a high-resolution monotonic clock and
/// `expected_frame_ms` is the display's nominal frame duration (e.g. 8.333 at
/// 120 Hz). A non-positive `expected_frame_ms` keeps the previously reported
/// value, so a driver that only knows the cadence later still gets correct
/// dropped-frame attribution once it does.
#[no_mangle]
pub extern "C" fn js_frame_metrics_record(timestamp_ms: f64, expected_frame_ms: f64) {
    if !frame_metrics_enabled() {
        return;
    }

    let due = {
        let mut m = METRICS.lock().unwrap_or_else(|p| p.into_inner());
        m.record(timestamp_ms, expected_frame_ms);

        let interval = auto_report_interval();
        if interval > 0 && m.frames_since_report >= interval {
            let line = m.summary_line();
            m.reset_window();
            line
        } else {
            None
        }
    };

    // Printed after the lock is released. A device launch streams stderr over
    // USB, so this write can block; holding the metrics lock across it would
    // stall the display-link callback that is supposed to be measuring frames.
    if let Some(line) = due {
        eprintln!("{line}");
    }
}

/// Frames between automatic reports (`PERRY_FRAME_STATS_INTERVAL`, default
/// 300 — about 2.5 s at 120 Hz). `0` disables automatic reporting.
///
/// An iOS app has no exit at which to print a summary, and `onFrame` metrics
/// are not reachable from TypeScript, so without this a device run collects
/// numbers nobody can see. Each line covers the frames since the previous one,
/// which is what makes "p99 while I was scrolling" a meaningful reading.
fn auto_report_interval() -> u64 {
    report_interval_flag().load(Ordering::Relaxed)
}

fn report_interval_flag() -> &'static AtomicU64 {
    static FLAG: OnceLock<AtomicU64> = OnceLock::new();
    FLAG.get_or_init(|| {
        AtomicU64::new(
            std::env::var("PERRY_FRAME_STATS_INTERVAL")
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(300),
        )
    })
}

/// Set the automatic-report cadence in frames. `0` disables it.
///
/// Runtime-settable rather than env-only so a benchmark can widen the window
/// for a long workload, and so a caller driving the metrics directly can turn
/// the periodic output off without controlling the process environment.
#[no_mangle]
pub extern "C" fn js_frame_metrics_set_report_interval(frames: f64) {
    let frames = if frames.is_finite() && frames > 0.0 {
        frames as u64
    } else {
        0
    };
    report_interval_flag().store(frames, Ordering::Relaxed);
}

/// Drop the running interval baseline without discarding collected samples.
///
/// Call this around a deliberate display-link pause (backgrounding, an
/// intentional stop/start) so the gap is not attributed as one enormous frame.
#[no_mangle]
pub extern "C" fn js_frame_metrics_mark_discontinuity() {
    let mut m = METRICS.lock().unwrap_or_else(|p| p.into_inner());
    m.last_ts_ms = None;
}

/// Discard all collected samples and counters.
#[no_mangle]
pub extern "C" fn js_frame_metrics_reset() {
    let mut m = METRICS.lock().unwrap_or_else(|p| p.into_inner());
    m.reset();
}

/// Number of frame intervals recorded since the last reset.
#[no_mangle]
pub extern "C" fn js_frame_metrics_count() -> f64 {
    let m = METRICS.lock().unwrap_or_else(|p| p.into_inner());
    m.total_frames as f64
}

/// Frames dropped since the last reset, derived from the driver-supplied
/// nominal frame duration.
#[no_mangle]
pub extern "C" fn js_frame_metrics_dropped() -> f64 {
    let m = METRICS.lock().unwrap_or_else(|p| p.into_inner());
    m.dropped_frames as f64
}

/// Longest single frame interval, in milliseconds.
#[no_mangle]
pub extern "C" fn js_frame_metrics_longest_ms() -> f64 {
    let m = METRICS.lock().unwrap_or_else(|p| p.into_inner());
    m.longest_ms
}

/// The nominal frame budget most recently reported by the driver, in
/// milliseconds (8.333 at 120 Hz, 16.667 at 60 Hz). `0` if no vsync has been
/// recorded yet.
#[no_mangle]
pub extern "C" fn js_frame_metrics_expected_ms() -> f64 {
    let m = METRICS.lock().unwrap_or_else(|p| p.into_inner());
    m.expected_ms
}

/// Percentile of the retained frame-interval window, in milliseconds.
/// `p` is in `[0, 100]`; `0` if nothing has been recorded.
#[no_mangle]
pub extern "C" fn js_frame_metrics_percentile(p: f64) -> f64 {
    let m = METRICS.lock().unwrap_or_else(|p| p.into_inner());
    percentile_of(&m.sorted_samples(), p)
}

/// Print a one-line summary to stderr. Returns the number of samples the
/// summary covered, so a caller can tell "all frames were fast" apart from
/// "no frames were measured" — the failure mode where a probe reports success
/// without its subject ever having run.
#[no_mangle]
pub extern "C" fn js_frame_metrics_report() -> f64 {
    let (line, n) = {
        let m = METRICS.lock().unwrap_or_else(|p| p.into_inner());
        (m.summary_line(), m.samples.len())
    };
    match line {
        Some(line) => {
            eprintln!("{line}");
            n as f64
        }
        None => {
            eprintln!("[frame-stats] no frames recorded (display-link driver never ticked)");
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The metrics store is global, so tests that touch it must serialize.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn fresh() -> std::sync::MutexGuard<'static, ()> {
        let guard = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        js_frame_metrics_set_enabled(1);
        // Automatic reporting resets the window, which would silently truncate
        // any test feeding more frames than the cadence.
        js_frame_metrics_set_report_interval(0.0);
        js_frame_metrics_reset();
        guard
    }

    /// Feed `count` vsyncs spaced exactly `step_ms` apart starting at `t0`.
    fn feed(t0: f64, step_ms: f64, count: usize, expected: f64) -> f64 {
        let mut t = t0;
        js_frame_metrics_record(t, expected);
        for _ in 0..count {
            t += step_ms;
            js_frame_metrics_record(t, expected);
        }
        t
    }

    #[test]
    fn percentile_is_nearest_rank_over_actual_samples() {
        let sorted: Vec<f64> = (1..=100).map(|v| v as f64).collect();
        assert_eq!(percentile_of(&sorted, 50.0), 50.0);
        assert_eq!(percentile_of(&sorted, 95.0), 95.0);
        assert_eq!(percentile_of(&sorted, 99.0), 99.0);
        assert_eq!(percentile_of(&sorted, 100.0), 100.0);
        // p0 and the empty case must not panic or index out of bounds.
        assert_eq!(percentile_of(&sorted, 0.0), 1.0);
        assert_eq!(percentile_of(&[], 50.0), 0.0);
    }

    #[test]
    fn first_vsync_establishes_baseline_without_recording_a_frame() {
        let _g = fresh();
        js_frame_metrics_record(1000.0, 8.333);
        // One timestamp is not an interval.
        assert_eq!(js_frame_metrics_count(), 0.0);
        js_frame_metrics_record(1008.333, 8.333);
        assert_eq!(js_frame_metrics_count(), 1.0);
    }

    #[test]
    fn steady_120hz_stream_reports_budget_and_no_drops() {
        let _g = fresh();
        feed(0.0, 8.333, 240, 8.333);
        assert_eq!(js_frame_metrics_count(), 240.0);
        assert_eq!(js_frame_metrics_dropped(), 0.0);
        assert!((js_frame_metrics_percentile(50.0) - 8.333).abs() < 1e-6);
        assert!((js_frame_metrics_expected_ms() - 8.333).abs() < 1e-6);
    }

    #[test]
    fn a_doubled_interval_counts_as_exactly_one_dropped_frame() {
        let _g = fresh();
        js_frame_metrics_record(0.0, 16.667);
        js_frame_metrics_record(16.667, 16.667);
        assert_eq!(js_frame_metrics_dropped(), 0.0);
        // One missed vsync: 2x nominal.
        js_frame_metrics_record(50.0, 16.667);
        assert_eq!(js_frame_metrics_dropped(), 1.0);
    }

    #[test]
    fn jitter_within_half_a_frame_is_not_a_drop() {
        let _g = fresh();
        // 60 Hz nominal, intervals wobbling by ±40% of a frame.
        let expected = 16.667;
        let mut t = 0.0;
        js_frame_metrics_record(t, expected);
        for i in 0..50 {
            t += if i % 2 == 0 { 22.0 } else { 11.5 };
            js_frame_metrics_record(t, expected);
        }
        assert_eq!(js_frame_metrics_count(), 50.0);
        assert_eq!(js_frame_metrics_dropped(), 0.0);
    }

    #[test]
    fn dropped_attribution_is_capped_so_one_hitch_cannot_dominate() {
        let _g = fresh();
        js_frame_metrics_record(0.0, 8.333);
        // A 10 s stall would otherwise attribute ~1200 dropped frames.
        js_frame_metrics_record(10_000.0, 8.333);
        assert_eq!(js_frame_metrics_dropped(), MAX_DROPPED_ATTRIBUTION as f64);
        // ...but the interval itself is still visible as the longest frame.
        assert!((js_frame_metrics_longest_ms() - 10_000.0).abs() < 1e-6);
    }

    #[test]
    fn discontinuity_suppresses_the_gap_but_keeps_history() {
        let _g = fresh();
        feed(0.0, 8.333, 10, 8.333);
        assert_eq!(js_frame_metrics_count(), 10.0);

        // App backgrounds for 4 seconds, driver marks the pause.
        js_frame_metrics_mark_discontinuity();
        js_frame_metrics_record(4_000.0, 8.333);

        // The gap produced neither a frame nor any dropped frames...
        assert_eq!(js_frame_metrics_count(), 10.0);
        assert_eq!(js_frame_metrics_dropped(), 0.0);
        // ...and the pre-pause samples survive.
        assert!((js_frame_metrics_percentile(50.0) - 8.333).abs() < 1e-6);

        // Recording resumes normally afterwards.
        js_frame_metrics_record(4_008.333, 8.333);
        assert_eq!(js_frame_metrics_count(), 11.0);
    }

    #[test]
    fn non_monotonic_timestamps_are_discarded_not_recorded_as_negative() {
        let _g = fresh();
        js_frame_metrics_record(100.0, 8.333);
        js_frame_metrics_record(50.0, 8.333); // driver bug: time went backwards
        assert_eq!(js_frame_metrics_count(), 0.0);
        // Baseline re-established at the later timestamp, so recording recovers.
        js_frame_metrics_record(58.333, 8.333);
        assert_eq!(js_frame_metrics_count(), 1.0);
    }

    #[test]
    fn ring_buffer_wraps_and_retains_the_most_recent_window() {
        let _g = fresh();
        // Fill with slow frames, then overwrite the whole window with fast ones.
        feed(0.0, 100.0, CAPACITY, 16.667);
        let t = (CAPACITY as f64) * 100.0;
        assert!((js_frame_metrics_percentile(50.0) - 100.0).abs() < 1e-6);

        feed(t + 1_000_000.0, 5.0, CAPACITY, 16.667);
        // Every retained sample is now from the fast run.
        assert!((js_frame_metrics_percentile(50.0) - 5.0).abs() < 1e-6);
        assert!((js_frame_metrics_percentile(99.0) - 5.0).abs() < 1e-6);
        // Cumulative counters still span both runs.
        assert_eq!(js_frame_metrics_count(), (CAPACITY as f64) * 2.0 + 1.0);
    }

    #[test]
    fn disabled_collection_records_nothing() {
        let _g = fresh();
        js_frame_metrics_set_enabled(0);
        feed(0.0, 8.333, 100, 8.333);
        assert_eq!(js_frame_metrics_count(), 0.0);
        // Re-enable so the flag does not leak into another test's run.
        js_frame_metrics_set_enabled(1);
    }

    #[test]
    fn automatic_reporting_resets_the_window_but_not_the_baseline() {
        let _g = fresh();
        js_frame_metrics_set_report_interval(10.0);

        // 25 frames at a 10-frame cadence: two reports fire, leaving 5.
        feed(0.0, 8.333, 25, 8.333);
        assert_eq!(js_frame_metrics_count(), 5.0);

        // The window still describes real frames — the baseline survived each
        // report, so the straddling interval was not dropped.
        assert!((js_frame_metrics_percentile(50.0) - 8.333).abs() < 1e-6);

        js_frame_metrics_set_report_interval(0.0);
    }

    #[test]
    fn a_zero_or_negative_report_interval_disables_automatic_reporting() {
        let _g = fresh();
        js_frame_metrics_set_report_interval(-5.0);
        feed(0.0, 8.333, 50, 8.333);
        // Nothing reset the window, so every frame is still counted.
        assert_eq!(js_frame_metrics_count(), 50.0);
    }

    #[test]
    fn report_returns_zero_when_no_frames_were_measured() {
        let _g = fresh();
        // Distinguishes "fast" from "never ran" — a probe that reports success
        // without its subject having executed is the failure mode this guards.
        assert_eq!(js_frame_metrics_report(), 0.0);
        feed(0.0, 8.333, 3, 8.333);
        assert_eq!(js_frame_metrics_report(), 3.0);
    }
}
